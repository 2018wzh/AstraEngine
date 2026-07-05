# Samples And Test Matrix

## Hard Samples

| Sample | Purpose | Required scenarios |
| --- | --- | --- |
| NativeVN Minimal | EngineCore + AstraVN smoke | boot、dialogue、choice、save/load、replay、package |
| NativeVN Commercial Baseline | 商业 VN 基线系统 | backlog、auto、skip、read-state、config、gallery、replay、route chart、voice replay、movie、transition |
| AstraVN Script Policy | 机制/策略分离样例 | [script sample](../samples/astra-vn-script/README.md)、Luau policy、Timeline/Fence、localization、system stories |
| TsuiNoSora Local Port | 真实项目压力样例 | full route、media coverage、release report、manual signoff |

## AstraEMU Family Samples

每个 family 使用用户本地合法数据，报告只提交 hash 和脱敏 metadata。v1 可用 family 是 Artemis；其他 family 输出 alpha probe report。实现顺序：Artemis、KrKr、BGI、SoftPAL、FVP、Siglus。

## Scenario Format

```yaml
schema: astra.scenario.v1
package: target/nativevn.astrapkg
seed: 42
actions:
  - wait_for: dialogue
  - advance: {}
  - wait_for: choice
  - choose: 0
assertions:
  - route: prologue.library
  - backlog_contains: "早上好。"
  - save_load_hash_match: true
```
