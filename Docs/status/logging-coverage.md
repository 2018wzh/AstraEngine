# Logging Coverage

机器可读真源是 [logging-coverage.json](logging-coverage.json)，准入脚本是 `Tools/check_observability.py`。脚本从 `cargo metadata` 读取当前 workspace，要求每个 member 都被分类，避免新 crate 无日志进入主线。

## 分类规则

- `instrumented`：运行时或工具链存在真实生命周期、决策、fallback 或失败边界，必须依赖 `tracing` 并发出带稳定 `event` 的事件。
- `not_applicable`：仅限纯 DTO/schema、proc-macro 或薄 facade；必须说明无法产生运行语义的具体原因。

当前覆盖 EngineCore/Runtime/Plugin、Asset/Package/Cook/Release、Media/Platform、AstraVN/Player、AstraEMU Manager/CLI/FVP/translation/package/evidence 以及 developer/fixture 链路。CLI native host 记录 provider/session/window/surface、fixed step 和受控 shutdown，不记录游戏目录或 overlay 状态；它与 Headless 使用相同的 family/RuntimeWorld 生命周期。AstraEMU 的事件只记录 provider/session/schema/count/hash/diagnostic，不记录扫描根、游戏正文、翻译正文、endpoint、model、secret 或商业 payload。`astra-observability` 本身不通过全局 subscriber 递归记录内部事件；queue saturation 由独立 critical path生成稳定 WARN。

性能 trace 与普通日志分离。`astra-observability` 的 Perfetto writer 只接收 safe domain/name、frame correlation、计数和耗时，使用固定事件缓冲和有界流式文件；正文、资源 payload、设备名、本地路径和 secret 不进入 trace。Headless product observer 把 package、Runtime/UI、Scene2D、WGPU 与内存/allocator counter 关联到同一 frame flow。trace 丢失、截断、时间戳回退或身份漂移由 `astra.performance_trace_manifest.v1` 阻断，不能降级为 WARN。

Windows native crash 只对 `astra-crash-reporter` 和 Windows bundled Player 生效。Android 的 GameActivity lifecycle、provider selection、JNI bridge 与 Player entrypoint 已列为 `instrumented`，但当前不声明 native dump；Web 与其他平台的 logging/ring/fatal tail 继续按各自 host 边界记录。

```bash
python Tools/check_observability.py
```
