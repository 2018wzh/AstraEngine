# Siglus 原生嵌入差异

## 精确来源

- 上游 `xmoezzz/siglus_rs`：`5f358ef1360b0439e92aed2b749e8f25d8721954`（1.0.0）。
- 本次从旧适配 `29fb9582751fc8ddfc23b374d428d6eba30bcb40` 初始化独占 submodule，保留该已发布历史。
- 通用原生基线：`63ec1f79ec7811f49ad1611668740a09e0d17cc7`，分支 `native-master-20261018`，直接建立于上述上游 revision。
- 唯一嵌入提交：`49457f433c24c4450c30b215e9518aff68499e29`，分支 `astra-embedded-20261018`，直接父提交为通用原生基线。主仓 gitlink 与 package metadata 使用此 revision。
- Kira 保留完整嵌套 submodule `vendor/kira`：0.9.6 上游 `dfb08d08d065930e9b257203bf11c6d34742f542`，`2018wzh/kira` 修复 `a299b42690094a71885f6ed97dc504c4038da9a7`。
- Kira 固定 CPAL 0.15.3 的 WASAPI 关闭修复 `65ac8a6ac1a635cc26bfc5af331295b052044908`（`2018wzh/cpal`）；不升级 Kira 主版本。
- 独立嵌入 manifest 固定共享 ABI `2018wzh/AstraEngine@94cd66ee9051d3643f51f4a61fed0466bf320280`。许可证及完整来源保留，见 `THIRD_PARTY_NOTICES.md`。

## 通用原生修复

- `platform_time` 的可选 `pausable-clock` 排除暂停时长，使用真实单调时钟，不启用 replay-only virtual-clock。`SiglusHost::set_suspended` 同步暂停原 Kira mixer 并清理输入状态。
- Kira 原 CPAL stream manager 持有 JoinHandle，启动握手返回设备错误；关闭先停设备、再 join。暂停作用于整个原 mixer，声音游标、track fade 和时钟不补跑；保留原设备切换路径。
- Kira `DecoderOwner` 与 `StreamingSoundData::with_decoder_owner` 保留原解码、混音及 playback-handle drop 语义。普通 owner 线程取消并 join 全部 decoder，某个 worker panic 不跳过其他 join；实时 callback 不执行 join。
- `AudioHub` 先关 device/mixer，再回收 decoder。MovieManager 原六类 worker 保留解码实现，接收端先断开再 join，避免有界发送阻塞关闭。被替换的语音 decoder 断开输出后保留 owner，完成时回收，关闭时全部 join，不把旧结果送入新语音。
- 原 WMV crate 版本 0.2.1、库名 `siglus_wmv_decoder` 及三个 CLI 的引用修复归入通用基线，避免与其他核心的同名产物冲突。

## 唯一嵌入适配

- `astra/embedded` 持有 C ABI、描述/探测、输入映射、原生 worker、typed 平台桥及不可变 CPU lease。主仓 crate 仅保留注册、构建依赖和源桥，删除 ProviderModule、Family advance、Host PCM 链。
- Dynamic `astra_emu_query` 与 Static `query` 返回同一 CoreApi。动态构建显式启用 `dynamic` 安装共享日志桥，静态构建复用宿主 subscriber；初始化先于 describe/probe/open。
- worker 调用原 `SiglusHost::new_with_renderer_sync` / `step`，保持原 proc-stack、renderer、Kira、电影、输入和 savedata。Host 不传 elapsed，也不推进 VM。
- 保留原离屏完整帧、sprite/wipe/shader/Emote/文字/系统 UI 及异步单 staging 回读。三个 Arc CPU 槽施加背压，不复制租约或扩为第四槽；五秒 map deadline 不因轮询或 resize 续期。
- CPU lease 独立于 session/device，关闭后仍有效，允许任意线程释放。身份包含 session/output/generation；原生成功 load/restart 更新 context，取消旧 map 和平台请求。
- L/R 使用原 Gameexe `SCREEN_SIZE`；保留原空配置 schema，typed 校验拒绝未知、重复和不支持的配置。窗口 O 改变不暗改 R。CPU 是唯一声明的输出路径，不声明 GPU sharing 或文字 Hook。
- 同进程单活动 session；pause/focus/suspend 走显式事件，cancel 独立唤醒平台等待，open 阶段检查宿主取消。close 消费句柄，停产并回收 worker；句柄查询不解引用已关闭 session。无窗口 Host 的原生 modal 明确返回 UNSUPPORTED。

## 验证入口与状态

本次未运行 build、lint、formatter、test 或真实运行；只解析并更新了依赖锁文件。旧 Family 的历史验证不代表新 C ABI 验收。

主线程统一运行 `cargo test -p astra-emu-siglus --test native_contract`、受影响库检查，以及显式硬件回读测试。动态库构建使用 `cargo build -p astra-emu-siglus --release --features dynamic`，实际加载、设备音频与窗口交互由新 Manager 验证。

真实 Rewrite+ 用例保留在 `tests/headless_game.rs`、`tests/headless_route.rs`、`tests/headless_playthrough.rs`。设置 `ASTRA_SIGLUS_TEST_GAME` 指向授权可写副本，并明确 `ASTRA_SIGLUS_TEST_WRITABLE_COPY=1`；不得使用原商业存档目录。代表流程为动画标题 → Start → 开场/影片 → 正文 → 原生保存/读取 → pause/focus/resume → close/reopen。保存测试检查真实数字槽变化及成功读档 context 换代；路线耗尽预算不算完成。自动化帧测试不替代可听音频和完整 UI 的人工观察。
