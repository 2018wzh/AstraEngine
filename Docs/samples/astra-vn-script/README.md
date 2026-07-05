# AstraVN Script Sample

本目录展示 AstraVN 脚本机制/策略分离的产品级样例。样例只用于文档说明，不表示 parser/runtime 已实现。

| 文件 | 内容 |
| --- | --- |
| [project.yaml](project.yaml) | 项目入口、策略包、系统页、本地化和发布锁定策略 |
| [main.astra](main.astra) | 主剧情、text key、choice、timeline、Luau policy command |
| [system.astra](system.astra) | title/config/gallery/replay/chart system stories |
| [standard_policy.luau](standard_policy.luau) | 官方标准策略包形态 |
| [cinematic_policy.luau](cinematic_policy.luau) | 第三方复杂演出策略包形态 |
| [full_playthrough.yaml](full_playthrough.yaml) | 商业级无头流程验收场景 |

验证文档链接：

```bash
python tools/check_docs.py
```
