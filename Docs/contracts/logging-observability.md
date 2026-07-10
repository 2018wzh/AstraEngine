# Logging 与 Crash Observability Contract

## 1. 边界

日志用于解释引擎如何运行，不参与 deterministic state、save、replay 或任何 hash。machine-readable report 继续写 stdout；日志写 stderr、显式相对日志目录，或平台 writable diagnostics 目录。Web 只提供 console、内存 ring 和 error tail，不声明本地文件或 minidump 能力。

Windows crash artifact 始终是 local-private 敏感数据，不进入 package、release report、Git 或自动上传。非 Windows 平台只实现 panic/fatal crash-tail contract，不冒充 native crash capture。

## 2. Host API

共享入口由 `astra-observability` 提供：

- `HostObservabilityConfig`：host role、filter、console format、file/ring limit 和 crash policy。
- `init_host(config)`：安装一次 host subscriber，返回 `ObservabilityGuard`。
- `ObservabilityGuard`：公开 session id、runtime filter reload、flush 和 fatal bundle 写入。
- `LogEventV1`：`astra.log_event.v1` 的稳定 DTO。
- `CrashBundleManifestV1`、`CrashArtifactRef`：fatal tail 与 crash artifact 引用 DTO。

库 crate 只发 `tracing` span/event。二进制 host 负责初始化 sink 和持有 guard，不能由库私自安装 subscriber。

## 3. 级别与事件所有权

| Level | 使用范围 |
| --- | --- |
| `TRACE` | tick/frame、队列、资源读取、provider 调用等高频细节 |
| `DEBUG` | 选择结果、映射、缓存和状态差异 |
| `INFO` | host/session/world/package/plugin/media/VN/platform 生命周期 |
| `WARN` | 允许继续的显式降级、fallback、队列丢弃汇总 |
| `ERROR` | 操作失败、blocking gate、ABI/provider/IO 不可恢复错误 |

每条事件必须有稳定 `event`；target 使用 crate/domain category。根因只在拥有处置权的边界记录一次 `ERROR`，中间层返回带上下文的 error，不逐层重复记录。昂贵字段只在对应 level enabled 时计算。

## 4. `astra.log_event.v1`

JSON file、critical file 和 ring 使用同一 schema，固定包含：

- `schema`、`timestamp`、`level`、`target`、`event`；
- `session_id`、`process_role`、`thread_label`；
- 有序 `span_stack`；
- 经审计的结构化 `fields`。

compact stderr 只供人读，不作为稳定交换格式。字段只允许 step、schema、hash、diagnostic code、provider/action/plugin id、状态和计数。禁止正文、payload、secret、native handle、原始环境值、绝对路径和未经审计的整体 `Debug` 输出。

## 5. Sink 与背压

- main JSON file 采用有界异步队列；默认单文件 16 MiB，保留当前文件和 8 个归档。
- WARN/ERROR 同步进入 critical ring/file，不能依赖 main queue 成功。
- ring 同时限制 4096 records 和 4 MiB。
- main queue 饱和时累计 `dropped_count`，并以 `observability.queue.saturated` WARN 写入 critical path 和 crash manifest。
- `flush` 必须等待已接收的 file records 落盘；不能用进程退出碰运气。

CLI 保留 `ASTRA_LOG`、`--log-filter`、`--log-format`、`--log-dir`，并提供 file size/archive limit 与 `--crash-dir`。CLI 未指定目录时不写文件。shipping Windows Player 默认使用平台 writable `Saved/Logs` 和 `Saved/Crashes`，默认 filter 为 WARN。

## 6. Windows crash reporter

`AstraCrashReporter.exe` 在 host 外运行。host 与 helper 通过预分配 shared request、ready/request/complete 同步对象握手；fatal panic 或未处理 SEH 只提交固定大小请求，dump 写入由 helper 完成。

helper 先写 `.partial`，成功后原子改名，再写 hash、size、crash-tail 和 manifest。dump flags 只允许 normal、filtered memory、thread info、unloaded modules、without optional data 和 filtered module paths；禁止 full memory、data segments、handles、token 和 private read/write memory。最多保留 10 份 crash bundle。

shipping Windows bundle 必须携带 role/hash/size 已登记且自检通过的 helper。`required` 握手失败或文件被篡改时阻断启动；`optional` 只允许开发配置显式启用，并发 WARN。

## 7. Bundle migration

`AstraPlayer.config.json` 和 standalone bundle manifest 使用 v2，包含 observability 配置、crash reporter role/hash 和 bundle checks。bundled Player 遇到 v1 必须输出 migration diagnostic 并要求重建，不能静默兼容。

## 8. 准入与验证

`Docs/status/logging-coverage.json` 对每个 Cargo workspace member 分类。`instrumented` crate 必须依赖 `tracing` 且存在稳定事件；纯 DTO、schema、proc-macro 或薄 facade 可以登记 `not_applicable`，但必须写明原因。入口检查：

```bash
python Tools/check_observability.py
cargo test -p astra-observability
cargo test -p astra-crash-reporter
```
