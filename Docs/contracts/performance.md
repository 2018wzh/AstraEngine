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

## Headless GPU E2

性能运行使用 `astra.headless_host_profile.v3`。`gpu_adapter` 必须声明 backend、device type、timestamp query 和可选 adapter identity；性能门禁拒绝 v2 迁移、软件 adapter、backend 漂移、device type 漂移和缺少 timestamp query。普通功能测试仍可显式迁移 v2，不能把迁移结果当成性能证据。

`astra-headless performance-e2` 固定执行 1,200 帧 warmup 和 72,000 帧 measurement，目标节拍为 120 Hz。warmup 会完成 atlas、pipeline、output texture 和 staging buffer 初始化，但不会进入统计 sample。预算至少覆盖 CPU/GPU/end-to-end 帧时、deadline miss、working set、private bytes、增长量、decoded cache、GPU resource/atlas、upload/readback、draw/queue/pipeline 和 allocator 指标。正式集显门禁每个 workload 独立执行三次，三份报告都要通过；独显只作对照，不参与放行。

产品运行通过 `run --performance-*` 接入同一 observer。`renderer-stress`、`product-stress` 与 `product-route` 使用不同预算种类：前两者要求稳定段 upload/readback 与逐帧 heap allocation 的 p95 为零；路线预算允许被 checkpoint、资源首次出现和 save/load 明确解释的有界活动，但仍执行相同的内存、帧时和 deadline 上限。package 校验、source unlock、Runtime/physical input、VN step、UI layout/paint、Scene2D、GPU submission、media decode/mix、save/load、checkpoint encode 和 artifact write 都进入同一 trace。Profiler 关闭时不读取时钟、不创建 writer或 profiling state；开启时使用固定容量事件编码、64 KiB 流式缓冲和有界 GPU timestamp ring，不在内存中积攒完整 trace。Release 构建按“启用 profiler 后增加的 p95 时间 / 8.333 ms 帧预算”计算开销，上限为 3%；额外 working set 上限为 32 MiB。

`astra.performance_trace_manifest.v1` 绑定 source revision、dirty state、build/package/profile/workload/session、adapter/driver hash、report hash 和 trace hash。事件丢失、截断、时间戳回退、空 trace 或任一身份漂移都阻断。原始 trace 只放 ignored 验证目录；仓库只保存字段定义和查询模板。

### Perfetto 字段

| category | slice/counter | 含义 |
| --- | --- | --- |
| `package.cpu` | `package.storage_verify`、`package.table_open`、`package.source_unlock` | bounded package 与 source-lock 路径 |
| `runtime.cpu` | `physical_input.consume`、`runtime.tick_action`、`vn.step`、`scene.build` | 物理输入、RuntimeWorld、VN 到 scene 的 CPU 时间 |
| `ui.cpu` | `ui.layout_paint` | Yakui layout、paint 和 Scene2D 命令生成 |
| `media.cpu` | `media.decode_mix` | decode、mixer 和完成 fence 处理 |
| `save.cpu` | `save_load` | save/list/load 事务 |
| `renderer.cpu` | `wgpu.prepare_submit` | WGPU prepare、queue submit 与 CPU staging |
| `renderer.gpu` | `atlas.upload`、`scene.pass`、`filter.pass` | encoder 内 timestamp query 得到的增量 atlas copy、scene 与 filter pass 时间 |
| `frame.flow` | `physical_input_to_gpu` | input → runtime → UI → SceneFrame → submission flow |
| `memory` | `working_set.bytes`、`private.bytes` | 进程内存 counter |
| `renderer` | `gpu_resource.bytes`、`atlas.bytes`、`upload.bytes` | GPU 驻留和传输 counter |
| `allocator` | `frame.bytes` | 引擎域逐帧分配 counter |

分析端使用外部 `perfetto-mcp==0.1.4`，不修改或依赖 `astra-mcp`。Trace Event JSON 可由 [Perfetto](https://github.com/google/perfetto) 导入；MCP 的版本和能力以 [perfetto-mcp 0.1.4](https://github.com/antarikshc/perfetto-mcp/blob/main/pyproject.toml) 为准。推荐先调用 `find_slices`，再用 `execute_sql_query` 聚合 inclusive time、单次大分配和稳定驻留：

```sql
SELECT cat, name, COUNT(*) AS samples,
       SUM(dur) / 1e6 AS inclusive_ms,
       MAX(dur) / 1e6 AS max_ms
FROM slice
GROUP BY cat, name
ORDER BY inclusive_ms DESC;

SELECT name, MAX(value) AS peak
FROM counter
GROUP BY name
ORDER BY peak DESC;

SELECT name, COUNT(*) AS samples, MAX(value) AS peak_bytes
FROM counter
WHERE name IN ('frame.bytes', 'gpu_resource.bytes', 'decoded_cache.bytes')
GROUP BY name
ORDER BY peak_bytes DESC;
```

## Migration、Release Gate 与测试

旧的无 identity timing JSON、静态 benchmark report 和未声明 metric 不迁移为 v1，读取时返回 unsupported/invalid report。Release Gate 只接受 `PerformanceStatus::Pass` 且通过 `validate_performance_report` 的同 run report；缺 report、blocked report、budget hash drift、sample 缺失或 identity drift 都是 blocking。

最小回归入口：

```bash
cargo test -p astra-core --test performance
cargo test -p astra-release --test release_report release_gate_accepts_only_measured_performance_from_the_same_clean_product_run
cargo test -p astra-platform-common --test audio_queue
cargo test -p astra-platform-windows --test media_session --features ffmpeg-vcpkg,platform-test-driver
```

这些测试覆盖预算/身份篡改、未知或超容量 sample、真实 FFmpeg→WASAPI/wgpu session、非静音 meter、seek、device loss、资源释放和 measured report。正式性能阈值仍需独立的 release-reference evidence。
