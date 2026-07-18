# TsuiNoSora 本地证据索引

本目录只保存迁移工具需要的公开模板与既有权威视觉证据。商业安装源、网络补充截图、转换产物、运行截图和差异图必须留在 ignored 私有工作区，不能写入 package、report 或 Git。

| 文件 | 用途 |
| --- | --- |
| `Title.png` | 标题画面权威视觉参考；由 `tsuinosora.visual_reference_report.v1` 校验固定尺寸和 hash |
| `Game.png` | 标准阅读画面权威视觉参考；由同一 report 校验固定尺寸和 hash |
| `demo.config.template.json` | 私有转换入口的无路径模板 |
| [original-ui-reference-audit.md](original-ui-reference-audit.md) | 1999 原版补充截图的来源、统一编号、hash、视觉分类和实现约束 |

补充截图统一放在 `Examples/TsuiNoSora/.local/work/original-ui-references/`。该目录受 `.gitignore` 保护，单张参考必须采用 `tsui1999-ui-NNN-<semantic-role>.png`，汇总图固定命名为 `tsui1999-ui-reference-contact-sheet.png`，索引固定命名为 `manifest.json`。文件名不得使用下载站标题、角色商业名称或本机路径。
