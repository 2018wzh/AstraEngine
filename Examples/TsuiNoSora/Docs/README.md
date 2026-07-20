# TsuiNoSora 本地证据索引

本目录只保存迁移工具需要的公开模板、脱敏 manifest 与既有权威视觉证据。商业安装源、全屏截图、裁剪图、转换产物、运行截图和差异图必须留在 ignored 私有工作区，不能写入 package、report 或 Git。旧桌面捕获严格裁剪固定的 1400×1050 游戏区域，再缩放为 800×600。第一线路新增捕获允许原生 800×600 RGBA，或带固定 1 px 边框的 802×602 RGBA；前者原样保留，后者只去除边框，两者都不缩放、不转换色彩。任一输入不满足自己的 manifest 契约都必须阻断。

| 文件 | 用途 |
| --- | --- |
| `Title.png` / `Game.png` | 既有公开基准；新增全屏捕获不再保存在本目录 |
| `original-ui-reference-manifest.json` | 15 个稳定 reference ID 的尺寸、hash 与裁剪参数；不含图片 payload |
| `demo.config.template.json` | 私有转换入口的无路径模板 |
| [original-ui-reference-audit.md](original-ui-reference-audit.md) | 1999 原版补充截图的来源、统一编号、hash、视觉分类和实现约束 |
| [classic-director-fidelity-design.md](classic-director-fidelity-design.md) | Director movie/handler/Score channel 到 Classic UI 与行为的映射 |
| [classic-first-route-private-rc.md](classic-first-route-private-rc.md) | Classic 第一线路 private research preview 的截图、Score snapshot、source unlock 与延期门禁 |
| `classic-first-route-recapture-manifest.json` | 新增 800×600/802×602 连续帧的脱敏 raw/normalized hash 与节点证据 |
| `classic-source-profile.json` | 1999 日文原版的 source verification profile；只含安全相对路径、长度与 hash |
| `classic-visual-node-map.json` | reference、Director 节点、typed state、wait occurrence 与 GPU checkpoint 的脱敏同节点映射 |
| `classic-visual-comparison-policy.json` | v3 固定几何/图像门禁、mask 上限和逐项色彩 tolerance 绑定 |
| `classic-visual-color-tolerance-approval.json` | 项目所有者明确批准的 `astra.headless_tolerance_approval.v2`；只授权固定 `capture_palette_v1`，不等同于 formal signoff |
| [original-game-debug-patcher.md](original-game-debug-patcher.md) | 1999 原版调试菜单、完整副本、自动日文转区和无边框窗口补丁器的契约与验收边界 |

补充截图统一放在样例 ignored 私有工作区的 `original-ui-references/` 生成目录。该目录分别保存 `raw/`、`cropped/`、`normalized/`，汇总图固定命名为 `tsui1999-ui-reference-contact-sheet.png`，索引固定命名为 `manifest.json`。迭代 run 可以使用版本后缀，但公开文档和 manifest 不依赖具体 run 目录。文件名不得使用下载站标题、角色商业名称或本机路径。
