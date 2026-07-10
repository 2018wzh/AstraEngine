# Logging Coverage

机器可读真源是 [logging-coverage.json](logging-coverage.json)，准入脚本是 `Tools/check_observability.py`。脚本从 `cargo metadata` 读取当前 workspace，要求每个 member 都被分类，避免新 crate 无日志进入主线。

## 分类规则

- `instrumented`：运行时或工具链存在真实生命周期、决策、fallback 或失败边界，必须依赖 `tracing` 并发出带稳定 `event` 的事件。
- `not_applicable`：仅限纯 DTO/schema、proc-macro 或薄 facade；必须说明无法产生运行语义的具体原因。

当前覆盖 EngineCore/Runtime/Plugin、Asset/Package/Cook/Release、Media/Platform、AstraVN/Player 以及 developer/fixture 链路。`astra-observability` 本身不通过全局 subscriber 递归记录内部事件；queue saturation 由独立 critical path生成稳定 WARN。

Windows native crash 只对 `astra-crash-reporter` 和 Windows bundled Player 生效。Web 与其他平台的 logging/ring/fatal tail 已纳入覆盖，但 native dump 仍是 `not_applicable`。

```bash
python Tools/check_observability.py
```
