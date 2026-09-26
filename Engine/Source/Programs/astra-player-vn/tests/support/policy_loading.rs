use super::*;
use astra_byte_source::{
    BoundedByteSource, ByteRange, ByteSourceError, ByteSourceStat, MemoryByteSource,
    RangeReadResult, SourceRevision,
};
use astra_player_vn::PreparedPolicyActions;
use astra_runtime::TaskScope;
use astra_vn_policy::{LuauPolicy, PolicyActionBinding, PolicyIoHost, PolicyReadText};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
const SOURCE: &str = r#"astra.command.register("note", {schema="note.v1",fields={}}, function() return astra.action.complete() end)"#;
fn package() -> Vec<u8> {
    product_package_with_request(STORY, |request| {
        let binding = PolicyActionBinding {
            descriptor: LuauPolicy::action_descriptors(SOURCE, Default::default())
                .unwrap()
                .remove(0),
            entry: "Policies/note.luau".into(),
            source: SOURCE.into(),
            source_hash: Hash256::from_sha256(SOURCE.as_bytes()).to_string(),
            read_text: vec!["asset:/text/note".parse().unwrap()],
        };
        request
            .extra_sections
            .push(astra_vn_package::policy_action_section(&[binding]).unwrap());
        request
            .extra_sections
            .push(astra_package::SectionPayload::raw(
                "asset.text.note",
                "text.v1",
                b"hello".to_vec(),
            ));
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&request.asset_vfs_manifest).unwrap();
        manifest["entries"].as_array_mut().unwrap().push(serde_json::json!({
            "vfs_uri":"package:/text/note.txt", "layer_id":"package.base", "source":{"kind":"package_section", "section_id":"asset.text.note"},
            "offset":0,"size":5,"hash":Hash256::from_sha256(b"hello"),"codec":"raw","media_kind":"text","diagnostics":[]
        }));
        request.asset_vfs_manifest = serde_json::to_vec(&manifest).unwrap();
        let mut catalog: serde_json::Value =
            serde_json::from_slice(&request.asset_catalog).unwrap();
        catalog["assets"].as_array_mut().unwrap().push(serde_json::json!({
            "asset_id":"asset:/text/note","vfs_uri":"package:/text/note.txt","media_kind":"text","tags":[],"profiles":["classic"]
        }));
        request.asset_catalog = serde_json::to_vec(&catalog).unwrap();
    })
}
struct CancelAfterText {
    inner: MemoryByteSource,
    offset: u64,
    armed: AtomicBool,
    reads: AtomicUsize,
    scope: TaskScope,
}
impl BoundedByteSource for CancelAfterText {
    fn stat(&self) -> Result<ByteSourceStat, ByteSourceError> {
        self.inner.stat()
    }
    fn read_range(
        &self,
        revision: SourceRevision,
        range: ByteRange,
        max: u64,
    ) -> Result<RangeReadResult, ByteSourceError> {
        let read = self.inner.read_range(revision, range, max)?;
        if self.armed.load(Ordering::SeqCst) && range.offset == self.offset {
            assert!(range.len <= astra_vn_policy::POLICY_IO_MAX_BYTES as u64);
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.scope.cancel();
        }
        Ok(read)
    }
}
#[test]
fn policy_preload_cancels_after_bounded_text_section_and_keeps_package_reusable() {
    let bytes = package();
    let reader = PackageReader::open(&bytes).unwrap();
    let offset = reader
        .container()
        .section_entry("asset.text.note")
        .unwrap()
        .offset;
    let content_root = reader.container().content_root();
    let source = Arc::new(CancelAfterText {
        inner: MemoryByteSource::new(bytes),
        offset,
        armed: AtomicBool::new(false),
        reads: AtomicUsize::new(0),
        scope: TaskScope::new(),
    });
    let reader = PackageReader::open_source(source.clone(), content_root).unwrap();
    source.armed.store(true, Ordering::SeqCst);
    let error = PreparedPolicyActions::load(&reader, 1024, &source.scope).unwrap_err();
    assert!(error.to_string().contains("POLICY_CANCELLED"));
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
    source.armed.store(false, Ordering::SeqCst);
    assert!(PreparedPolicyActions::load(&reader, 1024, &TaskScope::new()).is_ok());
}
#[test]
fn policy_preload_rejects_budget_and_runtime_never_reads_an_ungranted_id() {
    let bytes = package();
    let reader = PackageReader::open(&bytes).unwrap();
    assert!(PreparedPolicyActions::load(&reader, 5, &TaskScope::new()).is_err());
    let prepared = PreparedPolicyActions::load(&reader, 1024, &TaskScope::new()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert_eq!(
        runtime
            .block_on(prepared.read_text(PolicyReadText {
                id: 1,
                resource: "asset:/text/note".parse().unwrap(),
                max_bytes: 64
            }))
            .unwrap(),
        "hello"
    );
    assert!(runtime
        .block_on(prepared.read_text(PolicyReadText {
            id: 2,
            resource: "asset:/text/denied".parse().unwrap(),
            max_bytes: 64
        }))
        .is_err());
}
