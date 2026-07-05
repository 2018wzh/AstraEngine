# AstraEMU Artemis Core Blueprint

Artemis 是 AstraEMU v1 的首个可用 family。目标是通用 Artemis compat core：覆盖 PFS/PF6/PF8、boot、text/tag、legacy Lua bridge、presentation/media、snapshot 和 local case report。所有样例和报告只使用合法本地数据、synthetic fixtures 和脱敏 metadata。

## Process Boundary

```text
AstraEMU Manager
  -> local RPC: ProbeContent, LoadCase, Step, ApplyInput, SaveSnapshot
  -> shared memory: media block, decoded frame, audio chunk
  <- trace: TextCaptureEvent, PresentationCommand, AudioCommand, StateMachineTrace
```

Compat core 持有 Artemis VM 和权威状态机。Manager 不解析 core 私有内存，不接收商业 payload。

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

Probe 顺序：root marker、PFS header、PF6/PF8 index、patch chain、`system.ini`、BOOT entry、script/media distribution。

## Script Execution

Core 支持 `.iet` text/tag、`.ast` table row、`.asb` probe classification 和 legacy Lua bridge。`[lua]` block 和 `calllua` 是 Artemis legacy fact；AstraVN policy 仍使用 Luau。

```rust
pub enum ArtemisCommand {
    Text(TextCaptureEvent),
    Tag(TagCommand),
    CallLegacyLua { function: String, args_hash: Hash256 },
    Wait(AwaitToken),
    Presentation(PresentationCommand),
    Audio(AudioCommand),
}
```

未知 tag、未知 ASB branch、不可序列化 legacy state 必须输出 `DONE_WITH_CONCERNS` 或 `BLOCKED`，不能伪装通过。

## Snapshot

Snapshot section 最少包含 script stack、current entry hash、tag queue、serializable legacy Lua state allowlist、media state ref、save variables 和 diagnostics cursor。

## Checks

```bash
cargo test -p astra-emu-artemis artemis_pfs_probe
cargo test -p astra-emu-artemis artemis_script_tags
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/reports/artemis.yaml
cargo test -p astra-release emu_gate
```

Expected report: boot、text、choice、media command、save/load、snapshot replay、redaction policy 通过；报告不包含 key、完整脚本、截图、音频采样或私有绝对路径。
