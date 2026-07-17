use crate::NativeVnHostCommandSource;
use astra_asset::VfsUri;
use astra_core::Hash256;
use astra_media::{
    FontPackageEntry, FontPackageManifest, UnicodeRange, FONT_PACKAGE_MANIFEST_SCHEMA,
};
use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};
use astra_player_core::PlayerHostResourceId;
use astra_plugin_abi::{
    LoadPhase, PluginExtensionRegistrySnapshot, ProviderBinding, ProviderBindingContext,
    ProviderExtensionRecord, ProviderPolicy, PLUGIN_EXTENSION_REGISTRY_SCHEMA,
    PROVIDER_POLICY_SCHEMA,
};
use astra_vn_core::{compile_astra_project, AstraSource, CompileAstraProjectOptions, VnRunConfig};
use astra_vn_package::{package_sections_for_project, PLAYER_LOCALE_CONFIG_SCHEMA};
use astra_vn_runtime_provider::NativeVnRuntimeProvider;

const TEST_UI: &str = r#"
ui_bind surface:message view:ui.test.message controller:test.message policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.message
ui_bind surface:choice view:ui.test.choice controller:test.choice policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.choice
ui_bind system_page:title view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.title
ui_bind system_page:save view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.save
ui_bind system_page:load view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.load
ui_bind system_page:config view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.config
ui_bind system_page:backlog view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.backlog
ui_bind system_page:gallery view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.gallery
ui_bind system_page:replay view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.replay
ui_bind system_page:voice_replay view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.voice
ui_bind system_page:route_chart view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.route
ui_bind system_page:localization_preview view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.localization
ui_view ui.test.message model:astra.vn.ui_model.message.v1 theme:astra.vn.theme.classic #@id ui.test.message
  screen id:root
    panel id:advance fill:true
      on activate -> vn.advance
    text id:body value:$model.text_key
ui_view ui.test.choice model:astra.vn.ui_model.choice.v1 theme:astra.vn.theme.classic #@id ui.test.choice
  screen id:root
    virtual_list id:options items:$model.options item_key:option_id overscan:2 item_extent:48
      button id:option min_height:44 value:$item.text_key
        on activate -> vn.choose option_id:$item.option_id
ui_view ui.test.system model:astra.vn.ui_model.system.v1 theme:astra.vn.theme.classic #@id ui.test.system
  screen id:root
    button id:back min_height:48
      on activate -> vn.return_system
"#;

#[allow(dead_code)]
pub fn source_for(story: &str) -> NativeVnHostCommandSource {
    let package_bytes = product_package(story);
    let package = PackageReader::open(&package_bytes).unwrap();
    NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap()
}

pub fn product_package(story: &str) -> Vec<u8> {
    let compiled = compile_astra_project(
        [
            AstraSource::story("main.astra", story),
            AstraSource::ui("test-ui.astra", TEST_UI),
        ],
        test_compile_options(),
    )
    .unwrap();
    let mut sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let font =
        include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf")
            .to_vec();
    let font_hash = Hash256::from_sha256(&font);
    sections.push(SectionPayload::raw(
        "asset.font.ui",
        "astra.cooked_asset.v1",
        font.clone(),
    ));
    sections.push(SectionPayload::raw(
        "media.font_manifest",
        FONT_PACKAGE_MANIFEST_SCHEMA,
        serde_json::to_vec(&FontPackageManifest {
            schema: FONT_PACKAGE_MANIFEST_SCHEMA.to_string(),
            target: "nativevn-game".to_string(),
            profile: "classic".to_string(),
            provider_binding: "astra.vfs.package".to_string(),
            fonts: vec![FontPackageEntry {
                asset_id: "asset:/font/ui".to_string(),
                uri: VfsUri::parse("package:/fonts/ui.ttf").unwrap(),
                family: "Noto Sans JP".to_string(),
                face_index: 0,
                hash: font_hash,
                license_id: "OFL-1.1".to_string(),
                subset: Some("latin-basic".to_string()),
                coverage: vec![UnicodeRange {
                    start: 32,
                    end: 126,
                }],
                targets: vec!["nativevn-game".to_string()],
                profiles: vec!["classic".to_string()],
            }],
        })
        .unwrap(),
    ));
    sections.push(SectionPayload::raw(
        "vn.localization.en",
        "astra.vn.localization_table.v1",
        br#"{
            "schema":"astra.vn.localization_table.v1",
            "locale":"en",
            "strings":{
                "line":"Production line.",
                "line.after":"Line after timeline.",
                "choice.next":"Continue",
                "choice.end":"Finish",
                "system.back":"Back",
                "system.backlog":"Backlog",
                "speaker.hero":"Hero"
            }
        }"#
        .to_vec(),
    ));
    sections.push(SectionPayload::raw(
        "player.locale_config",
        PLAYER_LOCALE_CONFIG_SCHEMA,
        br#"{
            "schema":"astra.player_locale_config.v1",
            "default_locale":"en",
            "available_locales":["en"]
        }"#
        .to_vec(),
    ));

    let mut request = PackageBuildRequest::fixture("com.example.player.audio", "classic", sections);
    bind_product_provider_authority(&mut request);
    request.asset_vfs_manifest = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_vfs_manifest.v1",
        "prefixes": [{
            "prefix": "package",
            "provider_id": "astra.vfs.package",
            "backend": "package",
            "case_policy": "case_sensitive",
            "mode": "read_only",
            "redaction": "shipping",
            "capabilities": ["vfs.backend.package"]
        }],
        "layers": [{
            "layer_id": "package.base",
            "prefix": "package",
            "priority": 0,
            "source": {"kind": "package_section", "section_id": "package.manifest"},
            "targets": ["nativevn-game"],
            "profiles": ["classic"]
        }],
        "entries": [{
            "vfs_uri": "package:/fonts/ui.ttf",
            "layer_id": "package.base",
            "source": {"kind": "package_section", "section_id": "asset.font.ui"},
            "offset": 0,
            "size": font.len(),
            "hash": font_hash,
            "codec": "raw",
            "media_kind": "font",
            "diagnostics": []
        }],
        "whiteouts": []
    }))
    .unwrap();
    request.asset_catalog = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_catalog.v1",
        "assets": [{
            "asset_id": "asset:/font/ui",
            "vfs_uri": "package:/fonts/ui.ttf",
            "media_kind": "font",
            "tags": ["ui"],
            "bundle_id": "classic",
            "chunk_id": "base",
            "profiles": ["classic"]
        }]
    }))
    .unwrap();
    PackageBuilder::build(request).unwrap().into_bytes()
}

fn test_compile_options() -> CompileAstraProjectOptions {
    let mut theme = astra_ui_core::UiThemeManifest {
        schema: "astra.ui_theme_manifest.v1".into(),
        id: "astra.vn.theme.classic".into(),
        parent: None,
        tokens: [(
            "surface.system".into(),
            astra_ui_core::UiThemeValue::Color([0, 0, 0, 255]),
        )]
        .into_iter()
        .collect(),
        high_contrast_tokens: Default::default(),
        content_hash: Hash256::from_sha256(&[]),
    };
    theme.content_hash = theme.compute_hash().unwrap();
    let source = test_controller_source();
    let mut options = CompileAstraProjectOptions::default().with_ui_theme(theme);
    for id in ["test.message", "test.choice", "test.system"] {
        options = options.with_ui_controller_source(id, source.clone());
    }
    options
}

fn test_controller_source() -> String {
    r#"
local controllers = {
  { "test.message", "ui.test.message", "astra.vn.ui_model.message.v1" },
  { "test.choice", "ui.test.choice", "astra.vn.ui_model.choice.v1" },
  { "test.system", "ui.test.system", "astra.vn.ui_model.system.v1" },
}
for _, definition in controllers do
  astra.ui.controller.register(definition[1], {
    schema = "astra.vn.ui_controller.v1", view = definition[2],
    model_schema = definition[3], snapshot = "none",
  }, { on_action = function(_, _, action)
    return { astra.ui.effect.forward(action) }
  end })
end
"#
    .to_string()
}

fn bind_product_provider_authority(request: &mut PackageBuildRequest) {
    let specs = [
        ("presentation", "astra.renderer.wgpu", "renderer2d.wgpu"),
        ("vfs_provider", "astra.vfs.package", "vfs.backend.package"),
        (
            "game_runtime_provider",
            "astra.runtime.native_vn",
            "runtime.native_vn",
        ),
    ];
    let bindings = specs
        .iter()
        .map(|(slot, provider_id, capability)| {
            ProviderBinding::new(
                *slot,
                *provider_id,
                ProviderBindingContext {
                    package_id: request.package_id.clone(),
                    target: "nativevn-game".to_string(),
                    profile: request.profile.clone(),
                    required_capability: capability.to_string(),
                    engine_version: env!("CARGO_PKG_VERSION").to_string(),
                    rustc_fingerprint: "rustc-stable".to_string(),
                    feature_fingerprint: "runtime-envelope-v2".to_string(),
                    abi_fingerprint: "astra-plugin-abi-v2".to_string(),
                },
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    request.provider_policy = serde_json::to_vec(&ProviderPolicy {
        schema: PROVIDER_POLICY_SCHEMA.to_string(),
        profile: request.profile.clone(),
        renderer: "astra.renderer.wgpu".to_string(),
        decode_fallback: "profile_bound".to_string(),
        runtime_provider: NativeVnRuntimeProvider::descriptor(),
        bindings: bindings.clone(),
    })
    .unwrap();
    request.plugin_extension_registry = serde_json::to_vec(&PluginExtensionRegistrySnapshot {
        schema: PLUGIN_EXTENSION_REGISTRY_SCHEMA.to_string(),
        providers: specs
            .iter()
            .map(|(slot, provider_id, capability)| ProviderExtensionRecord {
                slot: slot.to_string(),
                provider_id: provider_id.to_string(),
                capability: capability.to_string(),
                phase: LoadPhase::Runtime,
                packaged: true,
                engine_version: env!("CARGO_PKG_VERSION").to_string(),
                rustc_fingerprint: "rustc-stable".to_string(),
                feature_fingerprint: "runtime-envelope-v2".to_string(),
                abi_fingerprint: "astra-plugin-abi-v2".to_string(),
            })
            .collect(),
        bindings,
        conflicts: vec![],
    })
    .unwrap();
    request.target_manifest = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.target_manifest.v2",
        "targets": [{
            "id": "nativevn-game",
            "kind": "game",
            "crate": "astra-vn",
            "runtime_provider": "native_vn",
            "ui_provider": "astra.ui.yakui",
            "default_profile": "classic",
            "platforms": ["windows", "web"],
            "packaged": true
        }]
    }))
    .unwrap();
    request.platform_eligibility = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.platform_eligibility.v1",
        "target": "nativevn-game",
        "profiles": ["classic"],
        "platforms": ["windows", "web"]
    }))
    .unwrap();
}
