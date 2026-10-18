# Siglus 原生嵌入差异

## 精确来源

- 上游 `xmoezzz/siglus_rs`：`5f358ef1360b0439e92aed2b749e8f25d8721954`（1.0.0）。
- 通用原生基线：`13b55b47484faf8dfe03deb6ffd56ca0ec4ba3f2`，保留于 `astra` 的历史；在原生生命周期和空栈后回标题修复之上移除商业硬件验收驱动。
- 单次嵌入适配：`f94c0dad6187844df1476381f473a7d395601b9b`，已发布到 fork 的 `astra`，直接父提交为上述通用基线。完整源码 tree 与 Release 构建的 `20262f4141b6737c98deca8f26cc203500d597a0` 相同；临时分支已清理，强推切换由用户明确批准，旧引用备份在本地 Git bundle。
- Kira 保留完整嵌套 submodule `vendor/kira`：0.9.6 上游 `dfb08d08d065930e9b257203bf11c6d34742f542`，`2018wzh/kira` 修复 `a299b42690094a71885f6ed97dc504c4038da9a7`。
- Kira 固定 CPAL 0.15.3 的 WASAPI 关闭修复 `65ac8a6ac1a635cc26bfc5af331295b052044908`（`2018wzh/cpal`）；Windows 宿主验证通过 workspace patch 使用兼容 ALSA 依赖的同版本 `ecfda841a9140d60bbec1630d47ca91f1a77b8f3`，不升级原 Kira 或替换设备后端。
- 独立嵌入 manifest 固定共享 ABI 2 `2018wzh/AstraEngine@15a39a626b1316cb6137a75e17e14a2ff2d1ba88`。许可证及完整来源保留，见 `THIRD_PARTY_NOTICES.md`。

## 通用原生修复

- `platform_time` 的可选 `pausable-clock` 排除暂停时长，使用真实单调时钟，不启用 replay-only virtual-clock。`SiglusHost::set_suspended` 同步暂停原 Kira mixer 并清理输入状态。
- Kira 原 CPAL stream manager 持有 JoinHandle，启动握手返回设备错误；关闭先停设备、再 join。暂停作用于整个原 mixer，声音游标、track fade 和时钟不补跑；保留原设备切换路径。
- Kira `DecoderOwner` 与 `StreamingSoundData::with_decoder_owner` 保留原解码、混音及 playback-handle drop 语义。普通 owner 线程取消并 join 全部 decoder，某个 worker panic 不跳过其他 join；实时 callback 不执行 join。
- `AudioHub` 先关 device/mixer，再回收 decoder。MovieManager 原六类 worker 保留解码实现，接收端先断开再 join，避免有界发送阻塞关闭。被替换的语音 decoder 断开输出后保留 owner，完成时回收，关闭时全部 join，不把旧结果送入新语音。
- 原 WMV crate 版本 0.2.1、库名 `siglus_wmv_decoder` 及三个 CLI 的引用修复归入通用基线，避免与其他核心的同名产物冲突。
- 原生空 proc-stack 进入空闲，不再留下永久 `paused` latch；redraw 随后排入的 `ReturnToMenu` 能继续执行。真实暂停仍走 `set_suspended` 的原时钟/音频路径。

## 嵌入适配

- `astra/embedded` 持有 C ABI、描述/探测、输入映射、原生 worker、typed 平台桥及不可变 CPU lease。主仓 crate 仅保留注册、构建依赖和源桥，删除 ProviderModule、Family advance、Host PCM 链。
- Dynamic `astra_emu_query` 与 Static `query` 返回同一 CoreApi。动态构建显式启用 `dynamic` 安装共享日志桥，静态构建复用宿主 subscriber；初始化先于 describe/probe/open。
- worker 调用原 `SiglusHost::new_with_renderer_sync` / `step`，保持原 proc-stack、renderer、Kira、电影、输入和 savedata。Host 不传 elapsed，也不推进 VM。
- 保留原离屏完整帧、sprite/wipe/shader/Emote/文字/系统 UI 及异步单 staging 回读。三个 Arc CPU 槽施加背压，不复制租约或扩为第四槽；五秒 map deadline 不因轮询或 resize 续期。
- CPU lease 独立于 session/device，关闭后仍有效，允许任意线程释放。身份包含 session/output/generation；原生成功 load/restart 更新 context，取消旧 map 和平台请求。
- L/R 使用原 Gameexe `SCREEN_SIZE`；保留原空配置 schema，typed 校验拒绝未知、重复和不支持的配置。窗口 O 改变不暗改 R。CPU 是唯一声明的输出路径，不声明 GPU sharing 或文字 Hook。
- 同进程单活动 session；pause/focus/suspend 走显式事件，cancel 独立唤醒平台等待，open 阶段检查宿主取消。close 消费句柄，停产并回收 worker；句柄查询不解引用已关闭 session。无窗口 Host 的原生 modal 明确返回 UNSUPPORTED。
- ABI 2 的 `module_owner` 初始化为空，原生失败不设置 `SESSION_CAN_RETRY`。补充 unsafe 查询的指针约束、消除 driver 的重复 logical 参数，并保持键码解析行为；Clippy 修复没有新增 suppress 或改变支持格式。

## Release 构建与人工验收

交付使用 `cargo build --manifest-path Emulator/Cargo.toml -p astra-emu-siglus --release --features dynamic` 生成的 `astra_emu_siglus.dll`。本次 MSVC x64 构建设置 `VCPKG_ROOT=D:/vcpkg-ffmpeg7` 和 `LIBCLANG_PATH=D:/LLVM/bin`。运行保留原 GPU 与 Kira/CPAL 音频，需 Windows 系统库、GPU 驱动及 VC Runtime。DLL 的 PE 导入表没有 FFmpeg 或其他 app-local 媒体 DLL；原 MPEG/WMV 解码 crate 静态链接进 family DLL。

本轮新增的 native ABI 验收入口、受控 stdin 路线、自动输入/截图 helper、adapter GPU 硬件用例及 Host 游戏硬件回归已移除；对应的 `libloading`、`tempfile` dev-dependencies 一并删除。上游既有普通逻辑测试、原生生产修复及诊断接口保留。

验收由用户在实际 Release 宿主中人工执行。构建成功只说明产物生成，不代表画面、输入、听音、存档或结局已人工验收。本轮清理后不启动 GUI/headless、不运行验收。
