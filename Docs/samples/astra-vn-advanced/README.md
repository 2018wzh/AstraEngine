# AstraVN Advanced Presentation Sample

本页保留 advanced presentation 的设计参考；Cook 入口已经合并到 `Examples/NativeVN`。旗舰项目的三条路线展示多层舞台、camera、video layer、shader/filter、voice sync、复杂 text effect、system story、system UI 和 release gate 的组合方式；普通商业 VN 发布不需要启用该 profile。

## Files

| 文件 | 用途 |
| --- | --- |
| [project.yaml](project.yaml) / `Examples/NativeVN/project.yaml` | Game target、profile 和 scenario refs |
| [main.astra](main.astra) / `Examples/NativeVN/Scripts/main.astra` | 多层 stage、camera、video、timeline、text effect、choice 和 system story |
| [system.astra](system.astra) | 旧设计参考；可执行 system story 已合并到 NativeVN canonical story |
| [advanced_policy.luau](advanced_policy.luau) | Timeline、fallback、shader/filter 和 text effect 策略参考 |

## Gate

```bash
astra cook Examples/NativeVN/project.yaml --profile advanced-vn --target nativevn-game --out target/nativevn-advanced-cooked
astra package build target/nativevn-advanced-cooked --target nativevn-game --out target/nativevn-advanced.astrapkg
astra package validate target/nativevn-advanced.astrapkg --profile advanced-vn --target nativevn-game --report target/reports/advanced-release.yaml
```

上述命令只覆盖 Cook/package 静态路径，不代表实际演出、输入、声音或路线通过。旧 YAML 执行样例不再保留，真实验证使用 NativeVN session 与对应物理输入。

Expected report fields: `vn.commercial_baseline`, `vn.system_ui_profile`, `vn.advanced_presentation`, `timeline.join_cancel`, `voice.sync`, `presentation.fallback`, `renderer.effect_budget` and player route checks.
