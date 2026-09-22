# AstraEditor

Editor 使用独立 Cargo workspace 和 GPUI 窗口。共享编辑逻辑位于 `astra-vn-editor`，不依赖 UI、模型服务或进程接口。

```sh
cargo run --manifest-path Editor/Cargo.toml -p astra-editor -- story.astra
```

入口接受已有 `.astra` 文件或 NativeVN `project.yaml`。工程模式同时载入剧情、UI、主题和 Controller；Content Browser 搜索源文件及声明的资源 sidecar，切换源文件保留未保存修改。文本输入、外部 Agent 写入和 MCP batch 都进入同一份未保存文档。Save 写入临时文件后原子替换；磁盘内容已变化时拒绝覆盖。关闭有未保存修改的窗口时，可以取消、保存或丢弃。

## 编辑与诊断

`AuthoringWorkspace` 保存文档版本，版本从 1 开始。`EditBatch` 内每个文档必须提供当前版本和不重叠的 UTF-8 字节范围；全部校验通过后一起应用。撤销和重做仍增加版本，旧 Agent 结果不能因内容恢复而重新生效。一个 batch 对应一次撤销。

`attribute_edit` 使用现有 CST 的 attribute span 修改源码，保留周围注释和 source ID。Outliner、剧情结构、演出视图和 Details 共用 source ID 选择；点击命令定位源码，Details 把一组属性修改作为单个 batch 提交。源码版本改变后旧 Details 提交会被拒绝。当前 Graph/Timeline 仍是可选中和编辑属性的命令投影，完整节点连线、拖拽关键帧和曲线编辑尚未完成。

左右面板可调整宽度，布局保存在 ignored `.astra-cache/editor-layout.json`；Reset layout 恢复默认。Ctrl/Cmd+S 保存全部源文档，源编辑区的 Undo/Redo 快捷键进入共用 batch 历史。Agent 输入和未提交的属性字段保留自己的局部输入撤销。

编辑后直接调用现有 `.astra` 编译器。诊断显示 source、行列和错误内容，并可跳转到输入位置。成功编译沿用 compiler source map，不生成另一套位置映射。

## 外部 Agent

```sh
cargo run --manifest-path Editor/Cargo.toml -p astra-editor -- story.astra --agent 'your-acp-agent'
```

Agent 命令和模型由用户配置，Editor 不持有模型 API 密钥，也不实现模型请求循环。ACP 使用官方 Rust SDK，外部进程通过客户端文件接口读取和修改当前打开的文件；没有 terminal 权限。每次写入必须先读取，写入绑定所读版本。

默认逐批确认：修改保存在待审查 batch，点击 Apply batch 后才应用；Reject batch 返回拒绝。自主模式直接提交经过相同校验的 batch。切换模式、取消任务或关闭 Editor 会使旧代次失效。取消会通知 ACP Agent，并关闭未及时结束的连接。未授权的其他路径和权限请求被拒绝。

`--mcp` 同时启动官方 rmcp stdio server，工具为 `read_document` 和 `apply_batch`。工具访问正在显示的同一份文档；后者接收 JSON batch 和读取时返回的 generation。逐批确认时，工具等待 UI 决定。stdout 属于 MCP 协议，诊断写 stderr。

## 产品预览

`--preview-config` 接收本地 JSON 配置，包含 `project`、`cli`、`player`、`profile`、`target`，Windows 可提供 `windows_runtime`。路径指向当前工作树自行构建的工具和工程，配置不应提交私有路径。

Save & Preview 先保存并编译当前文档，再执行现有 `cook → package build → package bundle → Player`。Player 创建独立 GPU 窗口，通过现有产品主路径使用真实 VnSession；Editor 没有另外实现 renderer。编辑版本改变、再次启动预览或点击 Stop preview 会终止旧进程，等待回收并清理临时输出。构建或 Player 失败显示退出状态和日志尾部。

当前入口没有片段 seek。它需要 Player 提供绑定编译身份、session generation、fragment source ID 和 presentation time 的 typed 控制接口；仅恢复当前片段 checkpoint，不能重新执行外部 IO。缺少 checkpoint、超出片段和旧代次必须返回错误，不能从头重放作为替代。

## 依赖与检查

GPUI 固定为 0.2.2，gpui-component 固定为与其匹配的 0.5.1；两者均为 Apache-2.0，组件库承担多行输入、IME、选择与光标行为。ACP 2.2.0 和 rmcp 3.4.0 使用官方 Rust SDK，同为 Apache-2.0。具体传递依赖由 `Cargo.lock` 固定。桌面目标为 Windows、Linux 和 macOS，各平台仍需分别运行验证。

```sh
cargo test -p astra-vn-editor
cargo test --manifest-path Editor/Cargo.toml -p astra-editor
cargo clippy --manifest-path Editor/Cargo.toml -p astra-editor --all-targets -- -D warnings
```

编译或协议测试通过不表示真实 GPU 预览或外部模型 Agent 已验收。当前阶段状态由主仓实施计划统一维护。
