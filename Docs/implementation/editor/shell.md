# Editor Shell

实现位于独立 `Editor` workspace 的 `astra-editor`。GPUI 管理窗口与事件，gpui-component 提供输入、Tree 和可调整面板。共享库 `astra-vn-editor` 负责版本化源文档和批量事务；UI、ACP 和 MCP 不维护各自的剧情副本。

工程装载读取现有 NativeVN project manifest 的 sources、ui_sources、ui_themes、ui_controllers 和 asset_roots。路径限制在工程内，重复文档、越界路径、损坏配置明确失败。浏览器切换文档保留各文档未保存内容，保存前检查磁盘冲突。

左侧 Content Browser 和 Outliner、中间源文档/剧情结构/演出视图、右侧 Details 共用 source ID 选择。Details 批量应用各属性的 CST value span，版本变化后拒绝旧面板内容。调整后的面板宽度保存到 ignored `.astra-cache`；Reset layout 恢复默认工作区。

Graph 从当前源文档的 state 和实际路由目标绘制连接，点击节点/路由定位同一份 Details。Timeline 从源 `keyframes` 属性派生时间刻度与帧选择，关键帧修改、插入、删除通过 `attribute_edit` 提交；它不保存另一份演出数据。关键帧拖动仅在原轨道释放时提交一次版本事务，取消不修改源码。Graph 连接端口创建经过目标校验的 jump，删除路由删除完整命令；源码编译继续负责可达性诊断。项目打开与预览配置使用 GPUI 系统文件选择器，项目切换处理保存、丢弃与取消，打开失败保留当前工程。

Agent 为辅助面板。每个 ACP 回合拥有一个有随机令牌的 loopback MCP 服务，与 stdio MCP 共用 EditorBridge。每次提交先检查文档版本，再按自主或逐批确认模式应用。外部权限面板展示关联工具详情；批准还必须匹配原 tool call、选项与 generation。取消推进 generation，待审查响应返回取消，旧 Agent 不能继续写入。

独立 Player 预览复用现有 cook/package/bundle。Editor 持有子进程并负责停止、回收和错误显示；公共 preview wire/checkpoint 契约由 Runtime/Player 维护。完整视觉编辑、曲线编辑、资源导入和跨桌面交互验收仍需继续实施，不能以已有面板数量宣称完成。

详见 [Editor 手册](../../../Editor/README.md) 和 [模块目标](../../modules/editor.md)。

工作区停靠直接使用 gpui-component 的 DockArea、Panel 与 dump/load，不维护第二套布局树。三个作者面板观察同一 Editor 状态，移动与组合标签不改变文档权威；布局为项目本地缓存，保存前后保留三个必要面板。损坏布局回到默认三栏并显示原因，完整桌面交互验收仍待进行。
