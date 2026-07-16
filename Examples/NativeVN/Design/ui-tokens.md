# UI tokens

Tokens 是内容包内部的稳定命名约束，不是第三方 UI 类型，也不是 Runtime package contract。实现时必须由目标平台的 UI backend 显式绑定，不能把 token 名称当作渲染完成证据。

| Token | 值 | 用途 |
| --- | --- | --- |
| `color.canvas.ink` | `#0B1220` | 场景和系统页的深色底 |
| `color.surface.glass` | `#172238` | 玻璃面板与浮层底色 |
| `color.text.primary` | `#F2F7FF` | 主要文字 |
| `color.text.muted` | `#A9B8CC` | 次要说明和不可用状态 |
| `color.signal.cyan` | `#63D8E6` | 当前焦点、信号和主要操作 |
| `color.signal.amber` | `#F0B96B` | 警示、未确认和授权待处理状态 |
| `space.unit` | `4px` | 间距基准 |
| `radius.panel` | `12px` | 面板和卡片 |
| `radius.control` | `8px` | 按钮和选择项 |
| `stroke.focus` | `2px` | 键盘/辅助输入焦点 |
| `motion.quick` | `120ms` | hover、focus 和轻量反馈 |
| `motion.scene` | `360ms` | 场景层级切换的建议默认值 |

文本、图标和背景的实际对比度必须在目标字体、字号、背景叠加和平台渲染路径上复核。没有真实平台 capture，不能把 token 表写成 UI E3。
