# AstraVN Script Sample

本目录展示 AstraVN 脚本机制/策略分离的产品级样例。当前真实 package/player gate 使用 `Examples/NativeVN`；本目录保留为文档样例，不作为 release evidence。

| 文件 | 内容 |
| --- | --- |
| [project.yaml](project.yaml) | 项目入口、Game target、策略包、系统页、本地化和发布锁定策略 |
| [main.astra](main.astra) | 主剧情、text key、choice、timeline、Luau policy command |
| [system.astra](system.astra) | title/config/gallery/replay/chart system stories |
| [standard_policy.luau](standard_policy.luau) | 官方标准策略包形态 |
| [cinematic_policy.luau](cinematic_policy.luau) | 第三方复杂演出策略包形态 |

验证文档链接：

```bash
python Tools/check_docs.py
```

旧 YAML 执行样例已删除。保留的源码用于说明业务语义，不等于可执行产品包；实际验证使用真实 VN session、Rust tests 与物理输入流程，见[开发手册](../../manual/development.md)。
