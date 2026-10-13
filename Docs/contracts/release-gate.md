# Release Gate Contract

`astra-release::ReleaseValidator` 校验 Engine 产品 package。Rust 类型与 profile 分支是报告和检查的真源；本页不为延后产品预设门禁。AstraEMU 自持 Family/Host，不进入该 package 发布链路。当前产品范围见 [重构契约](rebuild.md)，验收状态见 [实施状态](../status/implementation-plan.md)。

## Report Shape

`ReleaseReport` 包含 `schema`、`package_id`、`profile`、`status`、`package_hash` 和 `checks`。每条 `ReleaseCheckRecord` 包含：

| 字段 | Rust 类型与含义 |
| --- | --- |
| `id` | `String`，稳定检查标识 |
| `domain` | `ReleaseDomain`：runtime/target/plugin/package/media/scenario/platform/player/vn |
| `status` | `CheckStatus`：pass/warning/blocked |
| `summary` | `String`，检查摘要 |
| `diagnostic` | `Option<Diagnostic>`，结构化诊断；无值时 JSON 不输出该字段 |
| `evidence` | `Vec<ReleaseEvidence>`，每项为字符串 `key`/`value`，不是 map |

报告不额外定义 `source_ref`、顶层 `platform`、计数 summary 或 AI/RPG domain。源位置属于诊断的实际数据，不另复制一份报告 schema。

## Blocking Domains

现有检查覆盖容器完整性、schema/section、Cook 产物、target、机制插件/VFS binding、媒体与字体、VN 内容和系统 UI，以及调用方提交的性能、平台和 Player 证据。具体触发条件由实现决定，入口见 [检查实现](../implementation/release-gate-checks.md)。

`desktop-release` 和 `web-release` 要求 Cook 生成的 `compiled.project`；fixture package 不能冒充发布输入。字体声明为 required 时，必须从同一 package 的 font manifest 与 VFS 解析并校验，不能靠 loose file 或 system font 补足。性能报告须匹配 [Performance Contract](performance.md) 中的预算、样本及身份约束，编译、静态 benchmark 或短 smoke 不代表性能达标。

`player.full_playable` 消费真实平台输入与 Player 报告。接口存在、Headless 局部流程、静态画面、synthetic click 或绕过 event loop 的直接命令不能代表真实游戏流程通过。

平台验证直接消费 capability、host conformance 与 Player automation，不要求 Headless preflight 或具名审查。`HeadlessFormalEvidence`、review/link DTO、相关 CLI 参数和人工 signoff gate 已删除，不提供旧格式兼容入口。自动比较、容器完整性、真实输入、音频与平台身份校验保留；删除审批链不代表任何设备或路线自动通过。

`platform.headless_release_boundary` 阻止测试专用 Headless schema、profile 或 Developer binary role 进入 shipping package；该隔离检查不证明产品或测试 Host 已验收。

## Verification Commands

```sh
astra platform probe --platform windows --target nativevn-game --report target/platform-windows.yaml
astra package validate target/nativevn.astrapkg --profile desktop-release --target nativevn-game --platform-report target/platform-windows.yaml --report target/release_report.yaml
astra report explain target/release_report.yaml
```

这些命令需要真实输入，不能把 probe 或报告文件存在视为设备验收。旧 `astra test run --headless` 不再是执行入口；需要视听测试时使用独立 `astra-headless`，见 [操作手册](../manual/operator-guide.md)。
