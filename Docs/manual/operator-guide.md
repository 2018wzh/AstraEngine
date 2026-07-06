# Runtime / Platform Operator Guide

Operator 负责构建、打包、平台适配、Release Gate、crash bundle 和 AstraEMU local case report。

## 发布命令

```bash
astra cook project.yaml --profile desktop-release
astra package build target/cooked --out target/game.astrapkg
astra package validate target/game.astrapkg --profile desktop-release
astra test run scenarios/full_playthrough.yaml --package target/game.astrapkg --headless
```

## 日志命令

`astra` 默认把 machine-readable report 写到 stdout，把日志写到 stderr。需要结构化日志时使用：

```bash
astra test run scenarios/native_smoke.yaml --headless --format json --log-format json --log-filter astra_runtime=debug,astra_test=debug,astra_plugin=debug
```

需要落盘时显式传入相对目录：

```bash
astra test run scenarios/native_smoke.yaml --headless --log-dir target/logs
```

日志只用于排障，不参与 replay、hash、save 或 release 判定。

## 平台能力报告

每个平台模块必须输出 renderer、decode、audio、filesystem、input、save persistence、network 和 AI permission capability。Release Gate 根据 profile 判断是否可发布。

## Report Reference

| Report | 用途 |
| --- | --- |
| `astra.release_report.v1` | 发布资格 |
| `astra.scenario_report.v1` | 无头玩家流程 |
| `astra.platform_capability_report.v1` | 六平台能力 |
| `astra.plugin_report.v1` | 插件加载、卸载和 provider |
| `astra.emu.local_case_report.v1` | AstraEMU Artemis 和后续 family |

Release Gate check matrix 见 [Release Gate Checks Blueprint](../implementation/release-gate-checks.md)。
