# 性能与最小上游适配审计

更新：2026-10-17。范围为 RFVP、Siglus、Artemis 的 R01–R09 整改；CMVS、KrKr 不计本批适配验收。下一步见[实施计划](implementation-plan.md)，接口见[Family API](../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md)。

R01–R08 已实现并运行相关回归，三个 fork 的 `astra` 分支已发布。严格最小钩子内容仍依赖原生功能修复合并，R09 的 Sandbox 游戏验收受宿主启动崩溃阻塞。原版性能测试已按用户决定取消，保留原生功能归因与 Hosted 整改前后数据。历史构建过程和旧结论留在 Git。

## 固定核心基线

2026-10-17 核对的 upstream HEAD、单适配提交、远端分支与主仓 gitlink 如下；后续命名按[重构契约](../contracts/rebuild.md#外部核心与-sdk-的重构边界)统一使用 `astra`，主仓仍固定精确 revision，不跟随分支自动更新。

| 核心 | 上游 HEAD | `astra` 适配提交 | 差异规模 |
| --- | --- | --- | --- |
| RFVP 0.6.0 | `af557db750c7833274f22a6d82b0dc33cdea2f85` | `dcd39c23ca59b38a9a96b6f301aeb05121aa3e18` | 37 文件，+1662/−178 |
| Siglus 1.0.0 | `5f358ef1360b0439e92aed2b749e8f25d8721954` | `29fb9582751fc8ddfc23b374d428d6eba30bcb40` | 14 文件，+646/−74 |
| Artemis 0.4.0 | `ad90a62cb192b119b7cf8589aa7f87a94bd747c8` | `ae967d8446fe99b97f6e1df93fb1b61d8ada2040` | 18 文件，+1560/−279 |

规模包含清单、lockfile 与测试，不代表纯性能代码量。详细差异和来源见各 fork `astra` 适配提交内的 [FVP](https://github.com/2018wzh/rfvp/blob/95d3ee23548cb32e035aaaaeab2d309fc5d3020e/crates/astra-emu-rfvp/MODIFICATIONS.md)、[Siglus](https://github.com/2018wzh/siglus_rs/blob/04426d0384dd4e048b41fdd5b0508f0b92a77958/astra/MODIFICATIONS.md)、[Artemis](https://github.com/2018wzh/art3m1s-core/blob/5c83084d55573855713feefa949be24f0ae6baee/crates/astra-emu-art3m1s-core/MODIFICATIONS.md)；审计时的主仓包装已删除。来源、默认分支及已发布提交保留。

## 整改结果

| 项目 | 当前实现与关键证据 |
| --- | --- |
| R01 输入 | RFVP 在 VM 前每 tick 提交一次输入，删除重复映射。真实 portable/hosted HCB 覆盖按下、保持、repeat、松开、wheel、焦点与激活点击；portable 有首 tick 失败前/通过后记录。 |
| R02 RFVP 归因 | V1 修改像素/几何恢复、旧文字等待取消均有纯原生失败前/通过后证据。SaveCreate 的 hosted yield/capture_pending 保留为原 GPU 捕获边界；回归校验保存 PC、整帧、缩略图与恢复后继续。 |
| R03 Artemis 归因 | PF8 分隔符和 AVIO 所有权均有原生复现；AVIO 非法输入修复前崩溃，修复后成功关闭/提前停止/非法输入共 8 项通过。非对称 stereo PCM 在原 scalar 与 tuple 路径均正确，tuple 只作显式 stereo frame 表达；onLoad 修复独立提交。 |
| R04 有界回读 | Siglus 单 staging、Maintain::Poll、捕获 generation、固定五秒期限与取消；RFVP read-into 复用原代次/超时。Family 三槽在 map 前预留，持满返回 Pending，释放后无需 advance 即可重提，旧 lease 不变。硬件验证非对齐像素、resize、指针复用、abort/timeout/迟到回调与关闭。 |
| R05 必要差异 | 删除 RFVP 无消费者 sha2 与三条热路径 trace，保留 Family build.rs 的 sha2。原 read_exact_at 通过超 1 MiB、73-byte partial read、提前 EOF 与容量边界烟测；原生 scratch 栈数组使 100 次 build 的 4096-byte 分配从 100 次降为 0。 |
| R06 GPU 策略 | 原生 Any 默认排序保持不变；Family 显式 HardwareOnly 接受 Discrete/Integrated/Virtual，拒绝 CPU/未知。候选回归、实际 capture 与同进程三次重开通过；FFmpeg 声明恢复 9.0.0。 |
| R07 恢复时钟 | 完整成功 load/reset 产生一次通知，失败不抹除既有成功；Family 清 sub-ms remainder。Manager 等首帧 Ready/Unchanged 完成后重计时，普通 Pending 继续逻辑推进；七项调度/窗口/音频生命周期回归通过，elapsed 上限不变。 |
| R08 日志 | 仅精确 astra_emu::family event target 保留有界 typed 原值，普通 target 与 span 继续原 sanitizer。真实 DiagnosticSink→最终 JSON 覆盖 URL、路径、Debug、标量与 dropped_fields；超限拒绝，Manager 实跑的 wgpu 日志及像素正确。 |

两个 decoder 的非哈希库产物覆盖曾导致联合 Release LNK2019；Siglus 库名改为 `siglus_wmv_decoder` 后同一四包构建通过。原三个 CLI 均实际构建，自制 WMV2 解出五张 16×16 PNG；保留原解码器、features 和 VM 依赖别名。

## R09：Hosted 性能证据

设备为 RTX 4070 Laptop GPU、NVIDIA 610.88、Vulkan。商业场景 1280×720，自制静态对照 640×480；相同 Release 条件与独占游戏副本，整改前后各 15 组，每场景三轮。标题出现后至少预热 10 秒，采样 1800 次 advance、约 30 秒；只在采样窗外保存首尾 PNG。

下表为三轮分位数的最小值至最大值，单位 ms。`acquire 完成` 包含 Pending 轮询、map 完成、depad 与 ABI 返回，不等于纯 GPU 时间。

| 场景 | 构建 | advance p50 | advance p95 | advance p99 | acquire 完成 p50 | acquire 完成 p95 | acquire 完成 p99 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 《樱花萌放》动画标题 | 整改前 | 0.1991–0.2117 | 0.3332–0.4148 | 0.6559–0.8001 | 0.0023–0.0024 | 0.0042–0.0054 | 1.6738–1.7380 |
| 《樱花萌放》动画标题 | 整改后 | 0.2147–0.2161 | 0.3735–0.4936 | 0.7482–0.8151 | 0.0026–0.0029 | 0.0055–0.0073 | 1.0937–1.3147 |
| 《樱花萌放》开场演出 | 整改前 | 0.7011–0.7877 | 1.2984–1.6401 | 2.9014–5.3710 | 0.0040–0.0054 | 2.1482–5.9334 | 5.9210–13.1250 |
| 《樱花萌放》开场演出 | 整改后 | 0.7633–0.8319 | 1.4763–1.9059 | 2.6506–3.7290 | 0.0047–0.0066 | 2.0499–3.2245 | 3.6556–4.1669 |
| Rewrite+ 动画标题 | 整改前 | 3.0670–3.2331 | 4.9034–5.0484 | 5.9790–6.4961 | 1.7732–2.5473 | 7.2560–13.3751 | 8.5865–14.7970 |
| Rewrite+ 动画标题 | 整改后 | 2.7156–2.9571 | 3.6447–3.9320 | 4.1662–4.5230 | 0.6887–0.7632 | 0.9328–1.3882 | 1.2560–1.7296 |
| Rewrite+ 开场演出 | 整改前 | 1.9144–2.1612 | 2.9625–4.2885 | 4.1659–5.4683 | 1.7247–2.4410 | 3.4778–7.3928 | 4.6558–12.0048 |
| Rewrite+ 开场演出 | 整改后 | 1.7556–1.7942 | 2.4856–2.5516 | 2.7758–2.9695 | 0.6870–0.7403 | 1.0418–1.4948 | 1.2849–1.8666 |
| 自制 FVP 静态对照 | 整改前 | 0.0846–0.0897 | 0.1224–0.1318 | 0.1858–0.2132 | 0.0022–0.0023 | 0.0030–0.0032 | 0.0040–0.0045 |
| 自制 FVP 静态对照 | 整改后 | 0.0925–0.0991 | 0.1639–0.2223 | 0.2161–0.3441 | 0.0025–0.0028 | 0.0040–0.0054 | 0.0055–0.0076 |

Siglus 两场景的 advance 与回读完成分位数均下降；FVP 回读完成 p99 下降，但 advance p50 上升，静态对照三轮配对增量为 0.0028–0.0126 ms。数据支持回读长尾改善，未支持整体性能全面提升。测量开关开销仍未量化。

整改前后 Ready 数稳定为 70、835、1800、1799；自制静态窗无新 Ready，两个商业标题均有动画。开场截图为黑场到正文、月亮到光带，归类为演出而非纯正文或已确认电影。各轮退出 0，商业样本 NullAudio 有非零 PCM。原始计时、构建哈希和截图保留在 ignored 证据目录。

### 窗口交付与 GPU 完成

真实窗口发现 owner 仅随 VM tick 取帧，使异步 Siglus 接近隔帧发布；增加独立 1 ms 完成期限后，新图像能在逻辑 tick 间交付。两个观察窗的场景与时长不同，用于验证调度，不计算性能收益。

| 观察窗 | 秒 | advance | 新图像上传 |
| --- | ---: | ---: | ---: |
| 修复前启动演出 | 10 | 573 | 282 |
| 修复后 Rewrite+ 动画标题 | 30 | 1724 | 1714 |

[PresentMon 2.6.0](https://github.com/GameTechDev/PresentMon/releases/tag/v2.6.0) 的一次 30 秒窗口记录如下，单位 ms。GPU 指标包含核心与 Host 整个进程，GPU Time 含等待，不代表纯核心 timestamp。

| 指标 | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Present API CPU | 0.3731 | 1.4849 | 2.4126 |
| Present 至 GPU 完成 | 0.5103 | 1.7260 | 2.4516 |
| GPU Busy | 2.2796 | 3.4324 | 4.0091 |
| GPU Time（含等待） | 6.8634 | 14.0831 | 15.5593 |
| 两次 Present 间隔 | 14.3493 | 15.9857 | 19.9870 |

该窗口使用 Realtek 双声道 48 kHz/F32，稳定窗内队列空缺帧增量为零，正常退出；端点 loopback/听音仍未通过。系统默认 G431 仅提供八声道，原转换器明确拒绝。临时进程内设备选择与采样探针已移除，产品使用默认路径；后续计时使用 QPC 校准。

## 已验证范围

| Workspace | 通过 | 忽略 | 检查边界 |
| --- | ---: | ---: | --- |
| Engine | 871 | 22 | fmt、全目标严格 Clippy、build、完整 Cargo test |
| Editor | 18 | 3 | fmt、全目标严格 Clippy、build、完整 Cargo test |
| Emulator | 627 | 42 | 全部 workspace 包 fmt、全目标严格 Clippy、build、完整 Cargo test |

三个 workspace 的 build/Clippy/test 均退出 0；Manager include 模块单独按 Rust 2021 检查格式。Emulator 额外 `fmt --all` 暴露未修改上游 UEFI/音频模块的格式差异，保留原文。RFVP 原生完整 lib tests 仍受既有 texture 测试缺 `Read` 导入的 E0599 阻塞；相关入口由真实 rlib 链接行为回归运行，不删除原测试。

显式硬件运行包括 FVP lib 29 项、Siglus lib 六项及 Rewrite+ 输入/像素/PCM/关闭重开；Artemis 的硬件生命周期和恢复通知另有记录。最终去探针四包联合 Release 通过。FVP 自制 HCB/NVSG 运行 120 帧，精确验证 640×480 中的 4×2 白色及相邻黑色像素，原生 wgpu 日志完整，退出 0。普通测试、NullAudio 与硬件夹具各自报告，不替代商业验收。

### 游戏样本

| 样本与版本 | 已观察 | 待验收 |
| --- | --- | --- |
| FVP《樱花萌放》，发行/补丁修订未确认 | 硬件 GPU 标题、pointer 开始、开场至正文、非零 PCM、正常关闭 | 结局、完整菜单/槽位、设备听音和 Sandbox 流程 |
| Siglus Rewrite+，原版 Gameexe.dat / Scene.pck；附带汉化 V2 .chs 未启用 | 硬件 GPU 开场/动画标题、非零 PCM、global/config/read 写入与重开 | 当前菜单/槽位/代表流程、设备听音、GPU 警告归因 |
| ケロQ《終ノ空remake -2025ver-》Windows，tsuinosora_remake2025ver.exe，无 EXE 版本号 | 独立原生功能控制组启用现有媒体 API 后自然播放至 1600×1200 标题；Family 非商业 Vulkan/FFmpeg 回归 | 商业 Family 窗口/媒体/存读档/听音；headless typed 窗口拒绝只计错误边界 |

### 资源与环境限制

- Sandbox `AllowVGPU=1` 已实读，重建实例在产品与 guest 命令执行前消失；同时间 WER 为 `ContainerCrashDump`，`0x3B/0xC0000005`，未取得可用 dump。宿主 Windows 11 25H2 `26200.9711`、Sandbox `0.8.107.0` 与 [Microsoft #131](https://github.com/microsoft/Windows-Sandbox/issues/131) 的报告签名匹配，具体模块未在本机定位。此前软件 adapter 预检被明确拒绝；本批不以软件 GPU 或主机结果代替 Sandbox 验收。
- Siglus 退出 DXGI Live Object 警告和候选 D3D12 resource-state 错误仍待归因；短时内存平稳不足以判断长期泄露。
- 设备听音、商业长流程、四平台与发行依赖/许可仍未验收。Editor `--mcp` 持有 stdin 的关闭问题、Musica 旧逻辑尺寸和 CMVS 首 tick 接续点统一见实施计划。
- 运行证据保留在 ignored 区域。额外游戏副本与测试存档可按用户授权清理；后续运行重新创建隔离副本，保护原有游戏和既有存档。

## 上游修复

以下独立修复已发布，2026-10-17 核对均尚未合并；当前适配保留对应功能。合并后更新 upstream HEAD 并移除重复补丁。

| 原生修复 | 上游 PR | 独立提交 | 关键复现 |
| --- | --- | --- | --- |
| RFVP V1 图像/几何恢复 | [#51](https://github.com/xmoezzz/rfvp/pull/51) | `783d7d5712ad532e026d903e9f4b9151f2336ba9` | 两项上游失败，修复后像素与几何通过 |
| RFVP 旧文字等待取消 | [#53](https://github.com/xmoezzz/rfvp/pull/53) | `513b3955e2b2cb3b4a66944aed7e26d13de8acd9` | 退出后旧完成、非 TEXT 等待、restart reveal waiter |
| RFVP scratch 栈数组 | [#54](https://github.com/xmoezzz/rfvp/pull/54) | `cbe9f1fa609c9abea8e69417c266f3252945ebf5` | 原生 Release 分配计数对照 |
| Artemis onLoad 等待归属 | [#5](https://github.com/Alphaly2K/art3m1s-core/pull/5) | `ad20b8afd01ce2807510fa4e08629afc8e62345b` | helper 返回、保存 PC 等待及输入后继续；原生显式 GPU 回归 |
| Artemis PF8 分隔符 | [#7](https://github.com/Alphaly2K/art3m1s-core/pull/7) | `186ccc17fb10a6c6b3ee931d98a7c9d6830e722e` | 反斜杠索引以两种分隔符查找同一内容 |
| Artemis AVIO 所有权 | [#9](https://github.com/Alphaly2K/art3m1s-core/pull/9) | `348b931068d31bd75eb5ec804967ce4b86448aee` | 调试器定位 free_avio，修复后关闭/提前停止/非法输入通过 |
