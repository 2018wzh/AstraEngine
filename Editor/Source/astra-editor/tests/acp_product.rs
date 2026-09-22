//! Opt-in external ACP adapter test. Uses only a temporary, public source fixture.
use astra_editor::{
    agent::{AgentEdits, EditMode},
    bridge::{EditorBridge, Request},
};
use astra_vn_editor::{AstraSource, AuthoringWorkspace};

#[test]
#[ignore = "requires ASTRA_EDITOR_ACP_COMMAND and an authenticated external ACP agent"]
fn external_agent_edits_through_reviewed_document_transaction() {
    let command = std::env::var("ASTRA_EDITOR_ACP_COMMAND").expect("explicit ACP command");
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("main.astra");
    let original = "# public ACP editor test\nstory main #@id story.main\n";
    std::fs::write(&path, original).unwrap();
    let mut documents = AuthoringWorkspace::default();
    documents
        .open(AstraSource::story("main.astra", original))
        .unwrap();
    let mut edits = AgentEdits::default();
    edits.begin(EditMode::ReviewEachBatch).unwrap();
    let (bridge, mut requests) = EditorBridge::new();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let turn = astra_editor::acp::prompt(command, "Use astra_editor MCP read_document and apply_batch to change only the first comment to '# edited through ACP'. Preserve the story and source ID. Do not run shell commands or use other editing tools.".into(), path.clone(), bridge.clone());
        tokio::pin!(turn);
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(90));
        tokio::pin!(timeout);
        let mut applied = 0;
        let mut response = String::new();
        loop {
            tokio::select! {
                result = &mut turn => { result.unwrap(); break; }
                _ = &mut timeout => { bridge.cancel(); panic!("External ACP turn timed out"); }
                Some(request) = requests.recv() => match request {
                    Request::Permission { request, reply, .. } => {
                        use agent_client_protocol::schema::v1::{PermissionOptionKind, RequestPermissionOutcome, SelectedPermissionOutcome};
                        // The public fixture run permits only MCP approvals. Shell/file/network approvals stay denied.
                        let mcp = request.meta.as_ref().is_some_and(|meta| meta.get("is_mcp_tool_approval").and_then(|v| v.as_bool()) == Some(true));
                        let outcome = if mcp { request.options.iter().find(|option| option.kind == PermissionOptionKind::AllowOnce).map(|option| RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option.option_id.clone()))).unwrap_or(RequestPermissionOutcome::Cancelled) } else { RequestPermissionOutcome::Cancelled };
                        let _ = reply.send(outcome);
                    }
                    Request::Read { reply } => { let _ = reply.send(Ok(documents.document("main.astra").unwrap().clone())); }
                    Request::Apply { generation, batch, reply } => {
                        assert!(!edits.submit(generation, batch, &mut documents).unwrap());
                        assert_eq!(documents.document("main.astra").unwrap().text, original);
                        let result = edits.resolve(true, &mut documents);
                        if result.is_ok() { applied += 1; }
                        let _ = reply.send(result);
                    }
                    Request::AgentMessage { text, .. } => { if response.len() < 4096 { response.push_str(&text); } }
                }
            }
        }
        assert_eq!(applied, 1, "Agent did not use the Editor transaction bridge: {response}");
    });
    assert!(documents
        .document("main.astra")
        .unwrap()
        .text
        .starts_with("# edited through ACP\n"));
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        original,
        "Agent bypassed the Editor save boundary"
    );
    documents.undo().unwrap();
    assert_eq!(documents.document("main.astra").unwrap().text, original);
}
