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
