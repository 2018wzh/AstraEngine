# Release Observability

Release observability 包含 diagnostics、trace、profiling、scenario report 和 crash bundle。它服务发布判断，不替代 Editor debugger。

## Required Outputs

- `astra.release_report.v1`
- `astra.scenario_report.v1`
- `astra.runtime_trace.v1`
- `astra.plugin_report.v1`
- `astra.media_capability_report.v1`
- `astra.emu.local_case_report.v1`

Crash bundle 至少包含 release report、最近 structured logs、runtime trace tail、package manifest hash、plugin fingerprints、platform capability 和脱敏环境信息。

## Stage 1 Logging

Stage 1 CLI 已接入 `tracing`。库 crate 只发结构化事件；`astra-cli` 安装 subscriber，并保持 report 在 stdout、日志在 stderr。传入 `--log-dir` 时会额外写 rolling log，目录由命令行显式给出。

```bash
astra test run scenarios/native_smoke.yaml --headless --format json --log-format json --log-filter astra_runtime=debug,astra_test=debug,astra_plugin=debug
```

当前日志覆盖 Runtime tick、StateMachine action、plugin load/unload、FFI action、scenario run 和 report write。日志字段只包含 step、hash、schema、status、diagnostic code、provider id、action id、plugin id 和计数，不写 payload body 或本地绝对路径。

## Check Matrix

Release Gate 必须覆盖 runtime、plugin、package、media、VN、Editor、AI/MCP、platform、EMU 九个 domain。字段和命令见 [Release Gate Checks Blueprint](../implementation/release-gate-checks.md)。
