# Editor Shell

实现位于独立 `Editor` workspace 的 `astra-editor`。GPUI 管理窗口与事件，gpui-component 提供输入、Tree 和可调整面板。共享库 `astra-vn-editor` 负责版本化源文档和批量事务；UI、ACP 和 MCP 不维护各自的剧情副本。

工程装载读取现有 NativeVN project manifest 的 sources、ui_sources、ui_themes、ui_controllers 和 asset_roots。路径限制在工程内，重复文档、越界路径、损坏配置明确失败。浏览器切换文档保留各文档未保存内容，保存前检查磁盘冲突。

左侧 Content Browser 和 Outliner、中间源文档/剧情结构/演出视图、右侧 Details 共用 source ID 选择。Details 批量应用各属性的 CST value span，版本变化后拒绝旧面板内容。调整后的面板宽度保存到 ignored `.astra-cache`；Reset layout 恢复默认工作区。

Agent 为辅助面板。每次提交先检查文档版本，再按自主或逐批确认模式应用。取消推进 generation，待审查响应返回取消，旧 Agent 不能继续写入。

独立 Player 预览复用现有 cook/package/bundle。Editor 持有子进程并负责停止、回收和错误显示；公共 preview wire/checkpoint 契约由 Runtime/Player 维护。完整视觉编辑、曲线编辑、资源导入和跨桌面交互验收仍需继续实施，不能以已有面板数量宣称完成。

详见 [Editor 手册](../../../Editor/README.md) 和 [模块目标](../../modules/editor.md)。
