# Release Observability

Release observability 包含 structured logging、runtime trace、scenario/release report 和 local crash bundle。日志解释运行过程，report 才是 release gate 的机器证据；两者不能互相冒充。

## 当前实现

- `astra-observability` 已统一 host 初始化、category filter、runtime reload、compact/JSON console、限额 JSON file、critical mirror 和 fatal ring。
- `astra.log_event.v1` 固定 session/process role、span stack、thread label 和结构化 fields。
- 当前 Cargo workspace 的运行链路已按 [logging coverage](logging-coverage.md) 分类；纯 DTO/schema/proc-macro/facade 不制造无意义日志。
- `astra-cli`、`astra-player`、bundled Windows Player 和测试 host 共用配置入口。CLI 未给 `--log-dir` 时不落盘。bundled Player 在初始化日志前读取并严格校验 `AstraPlayer.config.json` 的 observability policy；配置中的日志、crash 目录只能是 bundle 内安全相对路径，显式 CLI 或 `ASTRA_LOG` 可用于受控验收覆盖。
- Windows bundle manifest 与 `AstraPlayer.config.json` 已升为 v2。shipping Windows bundle 携带经 role/hash/size 校验、自检和启动握手的 `AstraCrashReporter.exe`。
- 高频物理输入消费与 VN step 证据使用 TRACE。Windows E3 runner 显式启用 TRACE，并在 Player 运行期间持续、有界地排空 stderr；超过 16 MiB 时阻断，避免 pipe 背压把真实窗口输入路径卡死。runner 等待产品窗口标题，不会把 source-directory picker 误认成游戏窗口。该实现关闭日志风暴根因，但单个产品的 E3 状态仍须由同 build/package/input identity 的实际报告决定。
- Web host 输出同 schema 的 console/ring/error tail，不提供本地日志文件或 native minidump。

## Crash 边界

Windows helper 在进程外写 minidump，使用收敛的 filtered flags，先写 `.partial`，成功后原子改名并登记 hash/size。crash bundle 最多保留 10 份，始终是 local-private 敏感产物，不进入 package、report、Git 或自动上传。

其他平台只有 fatal tail contract；native crash capture 尚未实现，不能据此宣称平台 crash reporting 已完成。

## Required Outputs

- `astra.release_report.v1`
- `astra.scenario_report.v1`
- `astra.runtime_trace.v1`
- `astra.plugin_report.v1`
- `astra.media_capability_report.v1`
- `astra.log_event.v1`
- Windows local-private `astra.crash_bundle.v1`

## 验证

```bash
python Tools/check_observability.py
cargo test -p astra-observability
cargo test -p astra-cli --test logging
cargo test -p astra-player
cargo test -p astra-crash-reporter
```

字段、背压、隐私和 migration 规则见 [Logging 与 Crash Observability Contract](../contracts/logging-observability.md)。Release Gate domain 见 [Release Gate Checks Blueprint](../implementation/release-gate-checks.md)。
