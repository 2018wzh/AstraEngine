# SVG system icon set

图标由代码原生 SVG 构成，统一使用 `24 × 24` viewBox、`currentColor`、圆角线帽和可缩放几何路径。它们不依赖生成式位图、字体 glyph 或平台私有图标库。

| ID | 文件 | 语义 |
| --- | --- | --- |
| `title` | [title.svg](title.svg) | 标题/首页 |
| `message` | [message.svg](message.svg) | 消息/对白 |
| `choice` | [choice.svg](choice.svg) | 选择分支 |
| `backlog` | [backlog.svg](backlog.svg) | backlog |
| `save` | [save.svg](save.svg) | 保存 |
| `load` | [load.svg](load.svg) | 读取 |
| `config` | [config.svg](config.svg) | 配置 |
| `gallery` | [gallery.svg](gallery.svg) | 画廊 |
| `route` | [route.svg](route.svg) | 路线图 |
| `back` | [back.svg](back.svg) | 返回 |

实现时仍需在目标平台验证焦点、命中区域、对比度、缩放和 alt-text；SVG 文件存在本身不构成 UI E3。
