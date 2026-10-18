# Siglus 原生嵌入差异

## 精确来源

- 上游 `xmoezzz/siglus_rs`：`5f358ef1360b0439e92aed2b749e8f25d8721954`（1.0.0）。
- 本次独占验证从 `49457f433c24c4450c30b215e9518aff68499e29` 初始化完整 Siglus 与嵌套 Kira submodule，保留已发布历史。
- 通用原生基线：`63ec1f79ec7811f49ad1611668740a09e0d17cc7`，分支 `native-master-20261018`，直接建立于上述上游 revision。
- 当前单适配提交：`03dac5349c615ebbbbc65b3fa9d19b0977dcc81a`，分支 `codex/emu-native-single-adaptation-20261018`，直接父提交为通用原生基线。其源码 tree 与已验证的 `4779a967d4cb04bb3de252f2bdc20f41dac416bb` 相同；旧嵌入基线 `49457f433c24c4450c30b215e9518aff68499e29` 及已发布分支保留，不改写历史。主仓 gitlink 与 package metadata 使用当前单适配提交。
- Kira 保留完整嵌套 submodule `vendor/kira`：0.9.6 上游 `dfb08d08d065930e9b257203bf11c6d34742f542`，`2018wzh/kira` 修复 `a299b42690094a71885f6ed97dc504c4038da9a7`。
- Kira 固定 CPAL 0.15.3 的 WASAPI 关闭修复 `65ac8a6ac1a635cc26bfc5af331295b052044908`（`2018wzh/cpal`）；Windows 宿主验证通过 workspace patch 使用兼容 ALSA 依赖的同版本 `ecfda841a9140d60bbec1630d47ca91f1a77b8f3`，不升级原 Kira 或替换设备后端。
- 独立嵌入 manifest 固定共享 ABI 2 `2018wzh/AstraEngine@15a39a626b1316cb6137a75e17e14a2ff2d1ba88`。许可证及完整来源保留，见 `THIRD_PARTY_NOTICES.md`。

## 通用原生修复

- `platform_time` 的可选 `pausable-clock` 排除暂停时长，使用真实单调时钟，不启用 replay-only virtual-clock。`SiglusHost::set_suspended` 同步暂停原 Kira mixer 并清理输入状态。
- Kira 原 CPAL stream manager 持有 JoinHandle，启动握手返回设备错误；关闭先停设备、再 join。暂停作用于整个原 mixer，声音游标、track fade 和时钟不补跑；保留原设备切换路径。
- Kira `DecoderOwner` 与 `StreamingSoundData::with_decoder_owner` 保留原解码、混音及 playback-handle drop 语义。普通 owner 线程取消并 join 全部 decoder，某个 worker panic 不跳过其他 join；实时 callback 不执行 join。
- `AudioHub` 先关 device/mixer，再回收 decoder。MovieManager 原六类 worker 保留解码实现，接收端先断开再 join，避免有界发送阻塞关闭。被替换的语音 decoder 断开输出后保留 owner，完成时回收，关闭时全部 join，不把旧结果送入新语音。
- 原 WMV crate 版本 0.2.1、库名 `siglus_wmv_decoder` 及三个 CLI 的引用修复归入通用基线，避免与其他核心的同名产物冲突。

## 嵌入适配

- `astra/embedded` 持有 C ABI、描述/探测、输入映射、原生 worker、typed 平台桥及不可变 CPU lease。主仓 crate 仅保留注册、构建依赖和源桥，删除 ProviderModule、Family advance、Host PCM 链。
- Dynamic `astra_emu_query` 与 Static `query` 返回同一 CoreApi。动态构建显式启用 `dynamic` 安装共享日志桥，静态构建复用宿主 subscriber；初始化先于 describe/probe/open。
- worker 调用原 `SiglusHost::new_with_renderer_sync` / `step`，保持原 proc-stack、renderer、Kira、电影、输入和 savedata。Host 不传 elapsed，也不推进 VM。
- 保留原离屏完整帧、sprite/wipe/shader/Emote/文字/系统 UI 及异步单 staging 回读。三个 Arc CPU 槽施加背压，不复制租约或扩为第四槽；五秒 map deadline 不因轮询或 resize 续期。
- CPU lease 独立于 session/device，关闭后仍有效，允许任意线程释放。身份包含 session/output/generation；原生成功 load/restart 更新 context，取消旧 map 和平台请求。
- L/R 使用原 Gameexe `SCREEN_SIZE`；保留原空配置 schema，typed 校验拒绝未知、重复和不支持的配置。窗口 O 改变不暗改 R。CPU 是唯一声明的输出路径，不声明 GPU sharing 或文字 Hook。
- 同进程单活动 session；pause/focus/suspend 走显式事件，cancel 独立唤醒平台等待，open 阶段检查宿主取消。close 消费句柄，停产并回收 worker；句柄查询不解引用已关闭 session。无窗口 Host 的原生 modal 明确返回 UNSUPPORTED。
- ABI 2 的 `module_owner` 初始化为空，原生失败不设置 `SESSION_CAN_RETRY`。补充 unsafe 查询的指针约束、消除 driver 的重复 logical 参数，并保持键码解析行为；Clippy 修复没有新增 suppress 或改变支持格式。

## 验证入口与状态

2026-10-18 在独占 Windows worktree、独立 6.949 GiB 完整游戏副本运行。复制排除原 `savedata`，所有保存均来自测试副本。GPU 为 NVIDIA GeForce RTX 4070 Laptop GPU、Vulkan，1280×720 原生 renderer；Kira 0.9.6/CPAL 原设备链保持启用。

- `cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-siglus`：普通 C ABI contract 通过；硬件用例默认忽略，不能算作通过。
- `cargo build --manifest-path Emulator/Cargo.toml -p astra-emu-siglus --release --features dynamic`：通过，实际加载该 release DLL。
- `cargo clippy --manifest-path Emulator/Cargo.toml -p astra-emu-siglus --all-targets --features dynamic -- -D warnings`：通过。原 Kira/eluna 的依赖 warning 保留，不加屏蔽。
- `cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-siglus --lib -- --ignored --test-threads=1 --nocapture`：3 项真实 GPU readback 测试通过，覆盖 padding/resize、三租约背压与复用、timeout/cancel、跨 renderer 生命周期租约。
- 同一 release DLL 的 `native_contract` 和 `headless_game` 通过，覆盖原生输入、pause/focus/suspend/resume、固定 R、旧 CPU lease、close/reopen。
- 原 Kira `sound::streaming::owner` 3 项测试通过，包含 panic 后仍 join 全部 decoder。命令使用原 nested manifest，并通过 `--config patch."https://github.com/2018wzh/cpal.git".cpal.path="<own>/Emulator/ThirdParty/cpal"` 选择上述实际设备源码。
- 受控 C ABI 运行已完成动画标题 → Start → 原生序章 → 开场影片 → 教室对白 → 游戏内 Q.Save/Yes → Q.Load/Yes → 继续 30 次对白输入。原生数字槽 `0211.sav` 为 280186 bytes，成功 load 的 context 从 1 换到 2；pause 时帧代次不变、resume 后递增。关闭耗时 0.103 s，原 session worker、音频及 decoder 清理成功；同驻留 DLL 重开到 epoch 42，旧 lease 内容保持不变。
- 自动化命令 `cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-siglus --test headless_playthrough native_rewrite_save_load_close_reopen -- --ignored --exact --nocapture --test-threads=1` 通过（471.76 s）。实际 release DLL 创建/更新数字保存槽，load context 1→2，随后继续 60 次剧情输入、close/reopen，并保持旧租约；保存和读取的游戏内确认帧均已留证。最后一次 `headless_game` 重跑通过。

设置 `ASTRA_SIGLUS_TEST_GAME` 指向授权可写副本，并明确 `ASTRA_SIGLUS_TEST_WRITABLE_COPY=1`。可设置 `ASTRA_SIGLUS_TEST_LIBRARY` 指向动态库，helper 将其驻留到进程退出；`ASTRA_SIGLUS_TEST_CAPTURE_DIR` 只能指向 ignored 私有证据目录。原始日志、帧及输入时间线保留在独占 worktree 的 `.work/siglus-verification`。

旧 fixture 把 motion/down/up 放在同一批、在序章尚无保存 UI 时点击错误的 y=558，因而未证明菜单和保存。当前 fixture 分离真实输入时间线，保留原 native clock，使用实际 1280×720 控件及游戏内确认按钮；不伪造 Host 模态响应。上述短流程不覆盖商业结局。桌面锁屏，未做交互窗口或听音验收。

路线观察入口 `native_rewrite_controlled_route` 从标准输入逐行接收 `wait MS`、`click X Y`、`key NAME MS`、`snapshot NAME`、`reopen`、`close`。它使用相同 C ABI、真实按键持续时间及原生渲染，输出每条输入完成标记、帧身份和 session 状态；截图仍写入上述 ignored 目录。操作者按实际画面判断选项和结局，测试关闭成功不代表结局通过。已移除把盲目输入后 `SESSION_FINISHED` 当作路线验收的旧测试。

### Rewrite 小鳥结局与原生回标题修复

2026-10-18 在独占 `AstraEngine-emu-siglus-ending-20261018` 树从无存档授权副本，经同一 ABI 2 动态库完成标题、序章、共通线、小鳥个人线及真实 ED staff roll。选择按实际画面操作，主要分支包括「好きなもの」「小鳥を誘う」「答えられない」「小鳥を追いかける」「小鳥についてく」「接近してブレードで切る」。使用真实 Ctrl、按键和鼠标；地图继续跳读通过游戏自身 HELP 说明的按钮开启，不改商业脚本。

固定 `4779a967d4cb04bb3de252f2bdc20f41dac416bb` 首次到达 ED 后停在黑屏，ABI 仍为 Running。相同 pin、绕过 C ABI 的独立 `SiglusHost::new_with_renderer_sync/step` 从真实自动存档复现：`seen02000` 第 15962 行、`vm_halted=true`、proc stack 为 `[ReturnToMenu]`、`pending_exit=false`。原生空栈 `paused` latch 阻止 redraw 随后排入的回标题 proc；它不是 `set_suspended` 的时钟/音频暂停。

通用修复 `04614f02b35f623cdd1cd95b271c3b6cc309673a` 删除该陈旧 latch，保留原 proc pump、游戏执行及真实暂停。对应硬件 regression `host::tests::idle_proc_stack_accepts_return_to_menu` 通过。此前 native lib-test 的旧 rewind fixture 仍直接赋值裸 receiver，独立测试修复 `f0bf0113960dddc95f97cee894f009d864fd87c2` 按既有 `JoinedReceiver::completed` 惯例修正。

修复后同一 ABI 2 路径从本轮真实快存重验尾段：小鳥战斗选择 → ED staff roll → **自动回标题** → 同驻留 DLL close/reopen。全部关闭成功，进程 exit 0；Gallery 已解锁，完成率 29%。证据保留在 `.work/siglus-ending`，包括 `ending-fixed-abi.log`、`direct-ending-untraced.log`、`return-menu-regression-passed.log` 和 `frames/fixed-ending-{roll,title,reopened}.png`。本轮游戏副本及测试存档已删除；原副本未写入。部分其他核心的短启动按主线程协调并行，本轮不作性能、实窗输入或听音结论。通用提交由主线程整理到新的原生基线，此记录不重写已发布 pin 历史。
