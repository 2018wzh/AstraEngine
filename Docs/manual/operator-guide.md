# Runtime / Platform Operator Guide

Operator 负责构建、打包、平台适配、Release Gate、crash bundle 和 AstraEMU local case report。

## 发布命令

```bash
astra cook project.yaml --profile desktop-release
astra package build target/cooked --out target/game.astrapkg
astra package validate target/game.astrapkg --profile desktop-release
astra test run scenarios/full_playthrough.yaml --package target/game.astrapkg --headless
```

## 平台能力报告

每个平台模块必须输出 renderer、decode、audio、filesystem、input、save persistence、network 和 AI permission capability。Release Gate 根据 profile 判断是否可发布。
