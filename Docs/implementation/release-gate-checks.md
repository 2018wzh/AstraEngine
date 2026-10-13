# Release Gate 检查实现

报告结构只在 [Release Gate Contract](../contracts/release-gate.md) 与 `astra-release` Rust 类型中维护。本页定位真实实现，不复制 DTO、完整 check matrix 或尚未实现的 AI/RPG 检查。

| 实现入口（`Engine/Source/Developer/astra-release/src/`） | 职责 |
| --- | --- |
| `lib.rs` | Report/Check/Evidence 类型、容器校验与验证入口组合 |
| `checks_target.rs` | target manifest、Cook project 身份与 Headless shipping 隔离 |
| `checks_plugin.rs` | section、provider policy 与机制插件绑定 |
| `checks_vfs/` | mount、catalog、包内资源和边界校验 |
| `checks_vn/` | typed VN 行为、内容基线、系统 UI 与演出 |
| `checks_tsui/` | 终之空私有迁移输入及 Classic/Modern profile |
| `checks_headless.rs` | 测试专用 profile/schema 的 shipping 隔离；不要求 Headless 审批 |
| `checks_player.rs` | 真实 Player 流程及输入证据 |

`validate_package` 校验 package；platform/Player 与 performance/presentation 入口追加各自的真实报告检查，再计算总状态。没有提交某类报告不意味着对应产品能力通过；正式发布仍按实际 profile 要求验证平台输入。

典型稳定标识包括 `package.integrity`、`package.cooked_project`、`target.manifest`、`vn.commercial_baseline`、`vn.system_ui_profile` 与 `player.full_playable`。完整标识和错误条件从实现读取，不另建立批准矩阵；Headless preflight 和人工 signoff 不再是检查项。

```sh
cargo test -p astra-release --test release_report
astra package validate target/nativevn.astrapkg --profile desktop-release --report target/release_report.yaml
```

自动检查不能替代真实 GPU、音频、存读档、退出和商业路线验收。当前缺口和已运行结果统一见 [实施状态](../status/implementation-plan.md)。报告不包含商业 payload、secret、native handle 或私有绝对路径。
