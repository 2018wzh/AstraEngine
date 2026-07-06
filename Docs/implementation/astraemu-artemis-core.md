# AstraEMU Artemis Family Plugin Blueprint

Artemis 是 AstraEMU v1 的首个可用 family。目标是通用 Artemis engine-native family plugin：覆盖 PFS/PF6/PF8、boot、text/tag、legacy Lua bridge、presentation/media、snapshot 和 local case report。所有样例和报告只使用合法本地数据、synthetic fixtures 和脱敏 metadata。

## Runtime Boundary

```text
AstraEMU Manager
  -> create RuntimeWorld
  -> enable Artemis family plugin
  -> open LegacyRuntimeProvider session
  -> register coarse StateMachine action adapter
  -> tick RuntimeWorld
  <- RuntimeEvent / PresentationCommand / AudioCommand / TextCaptureEvent
  <- LocalCaseReport / ReleaseReport
```

Artemis plugin 可以持有 family-private interpreter state，但推进必须通过 `LegacyRuntimeProvider.step` 和可序列化 effect list。Manager 不解析 private state，不接收商业 payload。`EMUCoreBridge` 不参与 v1 主路径。

## Runtime Provider Registration

```rust
pub struct ArtemisFamilyPlugin {
    pub descriptor: LegacyFamilyPluginDescriptor,
    pub runtime: ArtemisRuntimeProvider,
}
```

注册流程：

1. `PluginDescriptor` 通过 fingerprint、permission、feature 和 packaged eligibility gate。
2. `LegacyFamilyPluginDescriptor` 声明 Artemis family、PFS/PF6/PF8、`.iet`、`.ast`、`.asb` 和 legacy Lua bridge capability。
3. ExtensionRegistry 注册 `LegacyRuntimeProvider` 和 release check。
4. Manager 根据 project/case profile 显式启用 Artemis provider，不按加载顺序选择。

## Artemis Probe

```rust
pub struct ArtemisProbeReport {
    pub family: FamilyId,
    pub pfs_version: Option<PfsVersion>,
    pub entry_count: u32,
    pub boot_script: Option<EntryHash>,
    pub script_kinds: Vec<ScriptKind>,
    pub media_kinds: Vec<MediaKind>,
    pub diagnostics: Vec<Diagnostic>,
}
```

Probe 顺序：root marker、PFS header、PF6/PF8 index、patch chain、`system.ini`、BOOT entry、script/media distribution。probe 只输出 hash、offset、entry count、format capability 和 diagnostic。

## Script Execution

Artemis provider session 支持 `.iet` text/tag、`.ast` table row、`.asb` probe classification 和 legacy Lua bridge。`[lua]` block 和 `calllua` 是 Artemis legacy fact；AstraVN policy 仍使用 Luau。

```rust
pub enum ArtemisActionEffect {
    Text(TextCaptureEvent),
    Tag(TagCommand),
    CallLegacyLua { function: String, args_hash: Hash256 },
    Await(AwaitToken),
    Presentation(PresentationCommand),
    Audio(AudioCommand),
}
```

未知 tag、未知 ASB branch、不可序列化 legacy state 必须输出 `DONE_WITH_CONCERNS` 或 `BLOCKED`，不能伪装通过。

## Snapshot

Snapshot section 最少包含 script stack、current entry hash、tag queue、serializable legacy Lua state allowlist、media state ref、save variables 和 diagnostics cursor。section 使用 Astra package/save 容器和 postcard payload，不另开私有存档格式。

## Release Gate

```bash
cargo test -p astra-emu-artemis artemis_pfs_probe
cargo test -p astra-emu-artemis artemis_script_tags
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/reports/artemis.yaml
cargo test -p astra-release emu_gate
```

Expected report: `emu.legacy_runtime_provider`、`plugin.extension_registry`、boot、text、choice、media command、save/load、snapshot replay、Runtime replay hash 和 redaction policy 通过；报告不包含 key、完整脚本、截图、音频采样或私有绝对路径。
