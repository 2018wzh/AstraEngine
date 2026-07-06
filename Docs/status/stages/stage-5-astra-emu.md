# Stage 5 AstraEMU Work

Stage 5 实现旧 VN compat core。AstraEMU core 独立进程持有 legacy engine 权威状态机，Manager 通过本地 RPC 和 shared memory 接收 trace、media block、TextCaptureEvent 和 snapshot。v1 可用 family 是 Artemis，其他 family 输出 alpha probe report。本页是 planned target 清单，不表示实现已经存在。

## S5-MANAGER-01 Manager IPC 与 shared memory

**ID:** `S5-MANAGER-01`

**Goal:** 建立 Manager/Core 本地 RPC、shared memory channel、capability handshake 和 report channel。

**Depends On:** `Docs/contracts/astraemu-ipc.md`、`S1-CORE-01`

**Target Paths:** `AstraEMU/Source/Manager/astra-emu-manager/src/ipc.rs`、`AstraEMU/Source/CoreApi/astra-emu-core-api/src/lib.rs`、`AstraEMU/Tests/ipc_handshake.rs` planned target

**Steps:**

1. 定义 core launch request、capability descriptor、protocol version 和 feature flags。
2. 定义 trace、media block、TextCaptureEvent、snapshot 和 diagnostic message。
3. 建立 shared memory bounded buffer，不传递 Actor 指针、Editor widget 或 native handle。
4. 编写 handshake、version mismatch 和 bounded media block 测试。

**Done Evidence:** Manager 能拒绝不兼容 core，并输出 machine-readable diagnostic。

**Linked Test IDs:** `T-S5-MANAGER-01`

## S5-MANAGER-02 Core process lifecycle

**ID:** `S5-MANAGER-02`

**Goal:** Manager 控制 compat core 启动、健康检查、停止、crash bundle 和 snapshot export。

**Depends On:** `S5-MANAGER-01`

**Target Paths:** `AstraEMU/Source/Manager/astra-emu-manager/src/process.rs`、`AstraEMU/Source/Manager/astra-emu-manager/src/crash_bundle.rs`、`AstraEMU/Tests/core_lifecycle.rs` planned target

**Steps:**

1. 实现 core process spawn、ready timeout、heartbeat 和 graceful shutdown。
2. core crash 时收集脱敏 crash bundle，不包含商业 payload 或私有绝对路径。
3. 支持 snapshot request 和 local report request。
4. 编写 startup timeout、crash bundle redaction 和 shutdown 测试。

**Done Evidence:** core lifecycle 失败能被 release gate 复查，不泄露本机路径。

**Linked Test IDs:** `T-S5-MANAGER-02`

## S5-CORE-01 Family core common API

**ID:** `S5-CORE-01`

**Goal:** 定义 family core 公共接口：probe、boot、tick、input、snapshot、report 和 shutdown。

**Depends On:** `S5-MANAGER-01`

**Target Paths:** `AstraEMU/Source/CoreApi/astra-emu-core-api/src/family.rs`、`AstraEMU/Source/CoreApi/astra-emu-core-api/src/report.rs`、`AstraEMU/Tests/family_core_api.rs` planned target

**Steps:**

1. 定义 FamilyDescriptor、CaseProfile、ProbeReport、StateMachineTrace 和 LocalCaseReport。
2. 让 family core 输出 TextCaptureEvent、PresentationCommand、AudioCommand 和 snapshot section。
3. 统一 DONE、DONE_WITH_CONCERNS、BLOCKED failure classification。
4. 编写 API serialization、failure classification 和 report redaction 测试。

**Done Evidence:** KrKr、Artemis、BGI 等 family 共享同一 Manager contract。

**Linked Test IDs:** `T-S5-CORE-01`

## S5-ARTEMIS-01 Artemis family core

**ID:** `S5-ARTEMIS-01`

**Goal:** Artemis core 支持 PFS/PF6/PF8 probe、boot keys、`.iet` tag、legacy Lua call/filter、presentation/media command、snapshot 和 report。

**Depends On:** `S5-CORE-01`、`Docs/emu/artemis/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-artemis/`、`AstraEMU/Tests/artemis/`、`scenarios/emu/artemis_full_flow.yaml` planned target

**Steps:**

1. 实现 PF6/PF8 header、index、entry bounds check、PF8 XOR 和 patch chain resolver。
2. 读取 `system.ini` boot keys，选择 platform section 和 BOOT entry。
3. 解析 `.iet` text/tag、legacy Lua block hash、`.ast` table row 和 ASB classification。
4. 接入 tag filter、enqueueTag、presentation/media command、AwaitToken 和 serializable snapshot allowlist。
5. 编写 synthetic PFS、boot metadata、tag parser、snapshot replay 和 full-flow scenario 测试。

**Done Evidence:** Artemis report 不含商业 payload、私有绝对路径、未授权截图、音频采样或完整脚本。

**Linked Test IDs:** `T-S5-ARTEMIS-01`

## S5-KRKR-01 KrKr family alpha profile

**ID:** `S5-KRKR-01`

**Goal:** KrKr core 在 v1 输出 alpha probe profile，验证 XP3 probe、virtual storage、script classifier、KAG boot trace、media bridge 和 release report。

**Depends On:** `S5-CORE-01`、`Docs/emu/krkr/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-krkr/`、`AstraEMU/Tests/krkr/`、`scenarios/emu/krkr_probe.yaml` planned target

**Steps:**

1. 实现 XP3 index、patch layering 和 virtual storage resolver。
2. 识别 KAG source、TJS bytecode、`.ks.scn`/PSB binary scenario，并为 unsupported branch 输出 diagnostic。
3. 输出 image、voice、BGM、movie command probe 和 boot trace hash。
4. 编写 synthetic fixture、metadata smoke 和 probe scenario 测试。

**Done Evidence:** KrKr alpha report 不含商业 payload、私有绝对路径、未授权截图或音频采样。

**Linked Test IDs:** `T-S5-KRKR-01`

## S5-BGI-01 BGI family core

**ID:** `S5-BGI-01`

**Goal:** BGI core 支持 PackFile/BURIKO ARC20、DSC decode、BCS/BP probe、VM memory、host dispatch、media probe 和 report。

**Depends On:** `S5-CORE-01`、`Docs/emu/bgi/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-bgi/`、`AstraEMU/Tests/bgi/`、`scenarios/emu/bgi_full_flow.yaml` planned target

**Steps:**

1. 实现 archive index、bounds check、name normalization 和 DSC decode。
2. 实现 BCS、BP、headerless scenario 检测顺序和 parser。
3. 实现 VM memory、stack、PC、program table 和 source map。
4. 实现 Host dispatch diagnostic、AwaitToken、Presentation、Image/Audio/Movie probe。
5. 编写 archive fixture、script fixture、VM dispatch 和 full-flow scenario 测试。

**Done Evidence:** BGI local report 只输出 hash、offset、entry count、opcode histogram 和脱敏 metadata。

**Linked Test IDs:** `T-S5-BGI-01`

## S5-SOFTPAL-01 SoftPAL 接入门槛

**ID:** `S5-SOFTPAL-01`

**Goal:** SoftPAL 在首批 family 稳定后接入，先完成 probe、resource catalog、script VM、extcall diagnostics 和 release gate。

**Depends On:** `S5-KRKR-01`、`S5-ARTEMIS-01`、`S5-BGI-01`、`Docs/emu/softpal/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-softpal/`、`AstraEMU/Tests/softpal/`、`scenarios/emu/softpal_full_flow.yaml` planned target

**Steps:**

1. 复用 family core API，不新增 Manager 私有通道。
2. 实现 PAC/DAT probe、resource catalog 和 script VM alpha route。
3. Unknown extcall 默认输出 diagnostic；presentation/audio/save/control-flow side effect 缺失时 release gate 不算通过。
4. 编写 fixture smoke、extcall report 和 full-flow scenario 测试。

**Done Evidence:** SoftPAL gate 能区分 recoverable diagnostic 和阻断玩家流程的 missing extcall。

**Linked Test IDs:** `T-S5-SOFTPAL-01`

## S5-FVP-01 FVP 接入门槛

**ID:** `S5-FVP-01`

**Goal:** FVP 在首批 family 稳定后接入，覆盖 probe、archive/media resolver、VM core、syscall mapper、presentation bridge 和 save/load。

**Depends On:** `S5-KRKR-01`、`S5-ARTEMIS-01`、`S5-BGI-01`、`Docs/emu/fvp/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-fvp/`、`AstraEMU/Tests/fvp/`、`scenarios/emu/fvp_full_flow.yaml` planned target

**Steps:**

1. 复用 family core API 和 release report schema。
2. 实现 pack/media resolver 和 HCB VM minimal execution。
3. 把 graph、text、sound、movie、thread syscall 转成 trace/event。
4. 编写 generated fixture、syscall mapper 和 full-flow scenario 测试。

**Done Evidence:** FVP report 能说明 syscall coverage，不提交商业脚本或媒体 payload。

**Linked Test IDs:** `T-S5-FVP-01`

## S5-SIGLUS-01 Siglus 接入门槛

**ID:** `S5-SIGLUS-01`

**Goal:** Siglus 在首批 family 稳定后接入，覆盖 root probe、Scene.pck、Gameexe、`.ss` script、G00/media 和 report policy。

**Depends On:** `S5-KRKR-01`、`S5-ARTEMIS-01`、`S5-BGI-01`、`Docs/emu/siglus/implementation-checklist.md`

**Target Paths:** `AstraEMU/Source/Families/astra-emu-siglus/`、`AstraEMU/Tests/siglus/`、`scenarios/emu/siglus_full_flow.yaml` planned target

**Steps:**

1. 复用 family core API 和 failure classification。
2. 实现 Siglus root、Scene.pck、Gameexe header 和授权 material 缺失 diagnostic。
3. 实现 `.ss` header、string table、label、operand decoder 和 basic stack model。
4. 实现 G00/Ogg/OVK/NWA/OMV probe，受保护 stream 只消费用户合法提供的材料。
5. 编写 probe-only report、script fixture 和 full-flow scenario 测试。

**Done Evidence:** Siglus report 不包含 key、payload transform、未授权截图或私有 stream。

**Linked Test IDs:** `T-S5-SIGLUS-01`

## S5-GATE-01 AstraEMU release gate

**ID:** `S5-GATE-01`

**Goal:** Release Gate 检查 Artemis full-flow scenario、local case report、trace、snapshot、TextCaptureEvent 和 redaction policy。

**Depends On:** `S5-CORE-01`、`S5-ARTEMIS-01`

**Target Paths:** `Engine/Source/Developer/astra-release/src/emu_gate.rs`、`Engine/Source/Developer/astra-release/tests/emu_gate.rs` planned target

**Steps:**

1. 增加 `emu.local_case_report`、`emu.artemis_full_flow`、`emu.report_redaction` 和 `emu.snapshot_replay` gate check。
2. 校验 report schema、hash、trace coverage、TextCaptureEvent 和 snapshot replay。
3. 校验报告不含商业 payload、未授权截图、音频采样、完整剧情脚本或私有绝对路径。
4. 编写 gate pass、missing trace blocked 和 redaction blocked 测试。

**Done Evidence:** 每个 family 都能用同一 release gate 输出脱敏 local case report。

**Linked Test IDs:** `T-S5-GATE-01`
