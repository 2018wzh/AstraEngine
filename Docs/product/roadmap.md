# 路线图

路线图采用 stage gate + 版本路线。每个阶段必须有入口条件、交付物、验收命令和退出标准。

## Version Train

| 版本 | 目标 | 退出标准 |
| --- | --- | --- |
| v0.1 EngineCore Alpha | Core、Runtime、插件 ABI、二进制容器、基础无头测试 | Native sample 可 package、headless run、save/load/replay |
| v0.2 AstraVN Alpha | 商业 VN 基线脚本和系统 UI | `.astra` sample 完成 dialogue/choice/backlog/auto/skip/save/load/config/video |
| v0.3 Editor Alpha | Qt/QML creator workflow | Project Wizard 到 Package/Release Gate 的闭环可用 |
| v0.4 AI/MCP Alpha | Runtime AI、Editor Copilot、Content Generation | Trusted session、Review Queue、provider-free replay 和 audit 通过 gate |
| v0.5 AstraEMU Alpha | KrKr、Artemis、BGI 首批 family core | 每个 family 产出脱敏 local case report 和 full-flow scenario |
| v1.0 Production | 桌面、移动、Web 发布矩阵 | NativeVN 与硬验收样例全平台通过 release gate |

## Stage Gates

### Stage 1：EngineCore

交付 RuntimeWorld、Actor/Component、StateMachine、EventBus、Scheduler、AwaitToken、Save/Replay、PropertySystem、Plugin ABI 和 headless scenario runner。

验收命令：

```bash
cargo test -p astra-runtime -p astra-save -p astra-plugin
astra test run scenarios/native_smoke.yaml --headless
```

### Stage 2：Media + Package

交付 Import/Cook、binary package、Renderer2D slot、TextLayout、AudioGraph、FilterGraph、DecodeProvider 和 release report。

### Stage 3：AstraVN

交付 `.astra` 编译、Lua extension、Graph/Timeline 同源、商业 VN 系统 UI、完整 playthrough scenario。

### Stage 4：Editor + AI/MCP

交付 Qt/QML editor、PIE、Inspector、Debugger、Package panel、Runtime AI、Editor Copilot、Content Generation 和 audit。

### Stage 5：AstraEMU

按通用性排序实现 KrKr、Artemis、BGI，再实现 SoftPAL、FVP、Siglus。每个 family 独立进程 core 必须输出状态机 trace、TextCaptureEvent、snapshot 和脱敏报告。
