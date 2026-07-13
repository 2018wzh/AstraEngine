# Performance Contract

性能证据不能靠 benchmark 名称、一次 wall-clock 打印或手写 JSON 生成。`astra-core` 的 `PerformanceBudget`、`PerformanceRecorder` 和 `PerformanceReport` 把预算声明、采样容量、run identity、统计结果与 blocking diagnostic 放在同一条 contract 中；各模块只负责提供真实测量点。

## Identity 与权限

`astra.performance_budget.v1` 必须绑定 `target`、`profile` 和 `profile_hash`。`astra.performance_report.v1` 还必须绑定 source revision、dirty state、package hash、build fingerprint 和 session id。身份字段只允许 safe symbol、Git revision 和 `sha256:`，不得写本地路径、用户名、设备名或 payload。

预算由 profile owner 声明，运行方不能在采样结束后放宽阈值。每个 metric 需要 unit、最少/最多 sample count，以及至少一个上限或下限；重复 metric、零容量、非单调阈值、未知 sample 和超出 sample capacity 都立即返回稳定的 `ASTRA_PERFORMANCE_*` 错误。Recorder 不参与 deterministic state、save 或 replay hash。

## 统计与失败语义

Recorder 保留有界整数 sample，finalize 后输出 min、p50、p95、p99、max 和 sample count。run duration 太短、sample 不足或阈值越界时，report 状态只能是 `blocked`，并携带 `ASTRA_PERFORMANCE_RUN_DURATION`、`ASTRA_PERFORMANCE_SAMPLE_COUNT` 或 `ASTRA_PERFORMANCE_THRESHOLD`。校验方会重新核对 budget hash、完整 identity、metric 集合、unit、sample count、percentile 单调性和阈值，不能把 blocked report 改写成 pass。

## Windows Native Media

`windows_media_performance_budget` 是当前第一个产品 budget owner。它同时接收 Windows host profile 和 package product profile：`profile_hash` 绑定 host 配置，`profile` 绑定产品 package。`WindowsNativeMediaSession::open` 强制接收 performance identity 和 budget；target、host profile hash 或 product profile 不一致时，在打开 decoder、surface output 或 audio session 前失败。Session 测量 open、tick、audio status、scheduler、present、pump、queue 深度、live payload bytes、WASAPI underflow、dropped video 和 device recovery，并在 shutdown 完成资源释放后生成 report。

WASAPI underflow 以一次 native callback 为单位，不再按缺失 sample 累加。公开 A/V fixture `flower-roar.mp4` 由同一 CC0 清单中的视频和非静音音频派生，用于同时验证视觉输出、真实 meter 和 A/V 恢复；仅有 sample count 而 peak 仍是静音不能算音频通过。

普通 debug test 只验证真实采样、身份连续性和 blocked/pass 两种语义。`ReleaseValidator::validate_package_with_product_evidence` 已要求 budget/report/capability/conformance/Player/package 共享 target、product profile、host profile hash、build fingerprint、package hash 和 session id，并阻断 dirty checkout、重复 budget、blocked/tampered report 与身份漂移。正式 E3 性能放行仍必须由 release build 在声明的参考环境运行；当前没有可提交的正式 pass artifact，因此不能把 validator 接线或 debug report 当成性能门禁已经关闭。

## Migration、Release Gate 与测试

旧的无 identity timing JSON、静态 benchmark report 和未声明 metric 不迁移为 v1，读取时返回 unsupported/invalid report。Release Gate 只接受 `PerformanceStatus::Pass` 且通过 `validate_performance_report` 的同 run report；缺 report、blocked report、budget hash drift、sample 缺失或 identity drift 都是 blocking。

最小回归入口：

```bash
cargo test -p astra-core --test performance
cargo test -p astra-release --test release_report release_gate_accepts_only_measured_performance_from_the_same_clean_product_run
cargo test -p astra-platform-general --test audio_queue
cargo test -p astra-platform-windows --test media_session --features ffmpeg-vcpkg,platform-test-driver
```

这些测试覆盖预算/身份篡改、未知或超容量 sample、真实 FFmpeg→WASAPI/wgpu session、非静音 meter、seek、device loss、资源释放和 measured report。正式性能阈值仍需独立的 release-reference evidence。
