# AstraVN Advanced Presentation Sample

本页描述 `Examples/AdvancedVN` 的 `vn.advanced_presentation` 验收入口。样例展示多层舞台、camera、video layer、shader/filter、voice sync、复杂 text effect、system story 和 release gate 的组合方式；普通商业 VN 发布不需要启用该 profile。

## Files

| 文件 | 用途 |
| --- | --- |
| [project.yaml](project.yaml) / `Examples/AdvancedVN/project.yaml` | Game target、profile 和 scenario refs |
| [main.astra](main.astra) / `Examples/AdvancedVN/Scripts/main.astra` | 多层 stage、camera、video、timeline、text effect 和 choice |
| [system.astra](system.astra) / `Examples/AdvancedVN/Scripts/system.astra` | save/load、config、backlog、gallery、replay、route chart、voice replay、localization preview |
| [advanced_policy.luau](advanced_policy.luau) | Timeline、fallback、shader/filter 和 text effect 策略参考 |
| [advanced_playthrough.yaml](advanced_playthrough.yaml) / `scenarios/advanced_presentation.yaml` | scenario，覆盖 presentation、system UI、save/load、replay 和 release gate |

## Gate

```bash
astra cook Examples/AdvancedVN/project.yaml --profile advanced-vn --target advanced-vn-game --out target/advancedvn-cooked
astra package build target/advancedvn-cooked --target advanced-vn-game --out target/advancedvn.astrapkg
astra test run scenarios/advanced_presentation.yaml --package target/advancedvn.astrapkg --target advanced-vn-game --profile advanced-vn --headless --report target/reports/advanced-vn.yaml
astra package validate target/advancedvn.astrapkg --profile advanced-vn --target advanced-vn-game --report target/reports/advanced-release.yaml
```

Expected report fields: `vn.commercial_baseline`, `vn.system_ui_profile`, `vn.advanced_presentation`, `timeline.join_cancel`, `voice.sync`, `presentation.fallback`, `renderer.effect_budget` and player route checks.
