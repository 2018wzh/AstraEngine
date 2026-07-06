# AstraVN Advanced Presentation Sample

本样例是 `vn.advanced_presentation` 的文档级验收入口。它展示多层舞台、camera、video layer、shader/filter、voice sync、复杂 text effect、system story 和 release gate 的组合方式；普通商业 VN 发布不需要启用该 profile。

## Files

| 文件 | 用途 |
| --- | --- |
| [project.yaml](project.yaml) | Game target、profile、command provider、Luau policy 和 system story 绑定 |
| [main.astra](main.astra) | 多层 stage、camera、video、text effect 和 choice |
| [system.astra](system.astra) | save/load、config、backlog、gallery、replay、route chart、voice replay、localization preview |
| [advanced_policy.luau](advanced_policy.luau) | Timeline、fallback、shader/filter 和 text effect 策略 |
| [advanced_playthrough.yaml](advanced_playthrough.yaml) | scenario，覆盖 presentation、system UI 和 release gate |

## Gate

```bash
astra package build Examples/AdvancedVN --target advanced-vn-game --out target/advancedvn.astrapkg
astra test run scenarios/advanced_presentation.yaml --package target/advancedvn.astrapkg --target advanced-vn-game --headless --report target/reports/advanced-vn.yaml
astra package validate target/advancedvn.astrapkg --profile advanced-vn --target advanced-vn-game --report target/reports/advanced-release.yaml
```

Expected report fields: `vn.commercial_baseline`, `vn.system_ui_profile`, `vn.advanced_presentation`, `command.provider_binding`, `timeline.join_cancel`, `voice.sync`, `presentation.fallback`, `renderer.effect_budget` and `source_map.identity`.
