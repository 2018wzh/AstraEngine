# 全产品重构实施状态

基线：28f89d88。用户于 2026-09-12 确认重构范围，当前状态按实际代码与运行结果维护；历史 Stage 状态不代表新架构完成。

| 阶段 | 状态 | 尚需完成 |
| --- | --- | --- |
| 0 规则与测试 | 进行中 | 新宪章/契约与轻量文档检查已落地；EMU 独立 workspace 和 xtask 已接通；541 处普通测试已迁移，强制 Headless 宏与旧状态矩阵已删除 |
| 1 EMU 薄 API/FVP | 进行中 | typed 配置、可选翻译、驻留库与 FVP 编码适配已整合；待整合验证和真实游戏运行 |
| 2 SDK/Minori | 未完成 | 独立 astra-text、Minori archive/profile、Family session、实际视听与自有存档已整合；完整 opcode、动画/长媒体和真实游戏验收待完成 |
| 3 跨平台 EMU | 未完成 | 三桌面/Android Manager、核心与真实媒体运行 |
| 4 Engine/VN | 未完成 | 演出 tick 去除整会话克隆并修复排队存档；无包 World 与 typed Actor/Component 存档已接通；其余任务、可信 Luau、typed 产品主路径和 DSL 待完成 |
| 5 Editor/Agent | 未完成 | GPUI、文本/图/时间线、独立预览、ACP/MCP 与两种编辑模式 |
| 6 终之空 | 未完成 | 新 .astra 工程、Classic/Modern 37 路线及私有四平台包 |
| 7 整体验收 | 未完成 | 全活动产品检查、真实流程、固定场景性能与旧路径清理 |

## 验收安排

本环境优先做可执行的实现和自动测试；另一环境中的合法游戏源和设备于阶段验收时接入。Windows 承担终之空 37 路线和两款 EMU 各一结局；其他三平台执行代表流程。未接入设备的结果保持未验收。

性能目标为 VN 桌面 1440p120、移动 1080p60。记录实际设备与固定场景结果，EMU 按原生速率分别测核心与 Host。

## 当前验证记录

- 独占重构 worktree；并行子任务各用独立 worktree/target。
- 新文档检查 222 页通过；7 个链接/卫生回归测试通过。
- EMU 独立 workspace 全量 fmt/clippy/build/test 通过：169 项测试通过，3 项按 GPU/联网条件未执行；含 API v2、Manager、FVP 和 Minori CLI。
- xtask 与 Linux platform all-target clippy 通过；修复 default build 的 audio fault-injection feature 组合错误。
- 普通逻辑测试与独立文字库已整合：子任务验证包含 104 个普通测试、15 个独立文本测试和 5 个媒体适配测试。
- Engine 全量 fmt/clippy/build/test 复验通过：666 项测试通过、0 失败、9 项按原条件未执行。修复了显式 Headless fixture 生命周期、无效 timeline fixture 和日志队列时序测试；已删除旧矩阵检查。
- 演出改动子任务的 35 项测试、all-target clippy 和 Player VN 调用方编译通过；Minori archive/profile 子任务的 40 项测试与 clippy 通过。
- Minori session 整合后 core/CLI fmt/clippy/build/test 通过，45 + 10 项测试通过。公开 fixture 覆盖 PAZ/SC、实际画面、PCM、输入、存读档和取消/关闭；动态库构建由子任务验证。尚未支持的 opcode、ANI/SQZ session 播放、长媒体流式解码和 Android 注册保持未完成。
- Minori 音频渐变现进入 AMINSV02 自有存档，保存采样进度并按剩余时间恢复；实际 PCM 连续性、fade-out 停止边界、参数替换和未播放资源停止测试通过。最新 Minori/CLI 全 feature 50 + 10 项测试、clippy 和 fmt 通过；旧 AMINSV01 拒绝且不覆盖。
- RuntimeWorld 构造不再要求 package；产品身份由宿主首 tick 前显式附加，无包/无 FSM 的 typed 更新、tick 和保存恢复已验证。Runtime world v5 与 NativeVN save_blob v5 同步，旧格式拒绝。Engine 最新全量 fmt/clippy/build/test 通过，669 项测试通过、9 项按原条件未执行。
- 真实 GPU/商业游戏/其他平台验收仍未执行。
