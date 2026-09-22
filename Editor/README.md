# AstraEditor

Editor 使用独立 Cargo workspace 和 GPUI 窗口。共享编辑逻辑位于 `astra-vn-editor`，不依赖 UI、模型服务或进程接口。

```sh
cargo run --manifest-path Editor/Cargo.toml -p astra-editor -- story.astra
```

入口接受已有 `.astra` 文件或 NativeVN `project.yaml`。工程模式同时载入剧情、UI、主题和 Controller；Content Browser 搜索源文件及声明的资源 sidecar，切换源文件保留未保存修改。文本输入、外部 Agent 写入和 MCP batch 都进入同一份未保存文档。Save 写入临时文件后原子替换；磁盘内容已变化时拒绝覆盖。关闭有未保存修改的窗口时，可以取消、保存或丢弃。

## 编辑与诊断

`AuthoringWorkspace` 保存文档版本，版本从 1 开始。`EditBatch` 内每个文档必须提供当前版本和不重叠的 UTF-8 字节范围；全部校验通过后一起应用。撤销和重做仍增加版本，旧 Agent 结果不能因内容恢复而重新生效。一个 batch 对应一次撤销。

`attribute_edit` 使用现有 CST 的 attribute span 修改源码，保留周围注释和 source ID。Outliner、剧情结构、演出视图和 Details 共用 source ID 选择；点击命令定位源码，Details 把一组属性修改作为单个 batch 提交。源码版本改变后旧 Details 提交会被拒绝。Graph 按实际 state、jump、option、branch 和 call 绘制连接，点击节点或路由后可在 Details 修改属性。Timeline 按实际时间绘制关键帧，可修改时间/数值、插入和删除；时间排序、重复时间和至少两个关键帧的约束在事务前检查，标量语义由编译器诊断。所有编辑仍写回同一 `.astra`，保留注释，并共用批量撤销。

Open project 使用系统文件选择器，切换前处理未保存修改；打开失败保留原工程。Preview setup 可载入当前工程的产品预览配置。Graph 的连接端口可拖到目标 state 创建 jump，要求源 state 有 scene 且尚无控制流；目标必须唯一存在。Delete route 删除完整路由命令（branch 的两个出口一起删除），可能产生的不可达状态由编译诊断显示。Graph 目前仍采用固定网格，尚无节点布局拖动。Timeline 可在原轨道拖动关键帧，释放时提交一次事务，离开轨道释放即取消；一个手势对应一次撤销，过期版本和重复时间会拒绝。当前引擎 timeline 仅支持线性插值，未添加虚假的曲线属性；缩放和曲线编辑尚未实现。二维场景操纵器尚未实现，UE 易用性目标未达成。

工作区使用 gpui-component DockArea：Content & Outliner、Authoring、Details 可拖动停靠、组合标签页、调整分区和放大。布局保存在 ignored `.astra-cache/editor-layout.json`，重开项目恢复；Reset layout 恢复默认三栏。布局损坏、版本变化或缺失/重复作者面板时显示诊断并使用默认布局。三个必要面板不可关闭，避免丢失人工编辑入口；旧的仅宽度布局直接重建。停靠交互尚未进行桌面操作验收。Ctrl/Cmd+S 保存全部源文档，源编辑区的 Undo/Redo 快捷键进入共用 batch 历史。Agent 输入和未提交的属性字段保留自己的局部输入撤销。

编辑后直接调用现有 `.astra` 编译器。诊断显示 source、行列和错误内容，并可跳转到输入位置。成功编译沿用 compiler source map，不生成另一套位置映射。

## 外部 Agent

```sh
cargo run --manifest-path Editor/Cargo.toml -p astra-editor -- story.astra --agent 'your-acp-agent'
```

Agent 命令和模型由用户配置，Editor 不持有模型 API 密钥，也不实现模型请求循环。ACP 使用官方 Rust SDK，并要求 Agent 声明 HTTP MCP 能力。每个回合启动一个只监听 loopback 的临时 MCP 服务，通过随机 Bearer 令牌授权，把同一个 EditorBridge 传给外部 Agent。Agent 使用 `read_document` 读取当前未保存源，再用 `apply_batch` 提交预期版本和 generation；没有第二套文件写入路径。

默认逐批确认：修改保存在待审查 batch，点击 Apply batch 后才应用；Reject batch 返回拒绝。自主模式直接提交经过相同校验的 batch。外部 Agent 的权限请求单独显示工具详情与该 Agent 提供的选项；它不等同于源修改审批。人工编辑、切换模式、取消任务或关闭 Editor 会使旧代次失效，并拒绝迟到批准。取消通知 ACP Agent、关闭临时 MCP 服务，并回收未及时结束的连接。Editor 不提供 terminal 客户端能力；外部进程自身的 sandbox 由所选 Agent 配置。

`--mcp` 同时启动官方 rmcp stdio server，工具为 `read_document` 和 `apply_batch`。工具访问正在显示的同一份文档；后者接收 JSON batch 和读取时返回的 generation。逐批确认时，工具等待 UI 决定。stdout 属于 MCP 协议，诊断写 stderr。

已用官方 codex-acp 1.12.0 的真实回合验证读取、权限确认、MCP batch、逐批审批与撤销，测试确认 Agent 没有直接修改磁盘文件。该测试使用公开临时源，不替代实际窗口操作验收。复测需显式设置 `ASTRA_EDITOR_ACP_COMMAND`，运行 `cargo test --manifest-path Editor/Cargo.toml -p astra-editor --test acp_product -- --ignored`；模型配置和认证保留在外部 Agent 中。

## 资源导入

Content Browser 的 Import image 支持 PNG、JPEG、WebP。选择图片后确认项目相对目标路径、asset ID、许可和已配置的 sidecar 根目录；使用工程的 cook profiles。后台调用现有 astra-cook 图片 importer 解码和计算 hash，再写入源图片与 `.astra-asset.yaml`，重开工程仍可浏览。取消或切换项目后，迟到的解码结果不会写入工程。

重名路径或 asset ID 会拒绝导入，保留表单供改名或取消；不会覆盖现有资源。目标目录必须留在项目内，sidecar 必须位于 manifest 配置的 asset_roots。单文件工程需要先打开项目。导入立即写入资源文件，不属于 `.astra` 的 Undo 历史；图片通过 asset ID 在源属性中引用。当前没有音频/视频/字体导入表单和资源缩略图，导入窗口交互仍待桌面验收。

## 产品预览

`--preview-config` 接收本地 JSON 配置，包含 `project`、`cli`、`player`、`profile`、`target`。Windows 还必须提供 `windows_runtime`（匹配的 Microsoft VC x64 CRT 目录）与 `crash_reporter`（构建的 AstraCrashReporter）。路径指向当前工作树自行构建的工具和工程，配置不应提交私有路径。

Save & Preview 先保存并编译当前文档，再执行现有 `cook → package build → package bundle → Player`。Player 创建独立 GPU 窗口，通过现有产品主路径使用真实 VnSession；Editor 没有另外实现 renderer。编辑版本改变、再次启动预览或点击 Stop preview 会终止旧进程，等待回收并清理临时输出。构建或 Player 失败显示退出状态和日志尾部。

预览使用 [typed Player 控制协议](../Docs/contracts/player-preview.md)。编译 project hash、全部文档版本/内容 hash 和启动 generation 固定到同一次 cook；只有 Player 返回 Ready 后才显示 Playing 与 Pause。暂停后显示当前片段实际保留的 checkpoint，可点击精确位置恢复，不重新执行剧情或外部 IO。没有 checkpoint 时不显示定位按钮。位置使用 Player 返回的当前 presentation time，不从检查点列表末尾推断。

Stop 请求真实 Player 关闭会话与设备，超过三秒仍未退出则回收子进程；文档改变、构建取消或编辑器关闭会回收旧进程。管道读写使用有界队列和有界 JSONL，子进程退出后 join 两个通信 worker。旧身份、控制拒绝、断管、启动失败与超时显示为错误。

公开最小工程已经通过真实 Windows Player 的 cook/package/bundle、Ready、Pause、当前片段精确 checkpoint 恢复、Resume 与正常 Stop。复测需设置 `ASTRA_EDITOR_PREVIEW_CONFIG`，运行 `cargo test --manifest-path Editor/Cargo.toml -p astra-editor --test preview_product -- --ignored`；它会启动 GPU 窗口。此测试检查产品进程与协议状态，不检查画面、音频质量或手动操作；NativeVN 旗舰工程、连续创作和窗口交互仍需验收。

## 依赖与检查

GPUI 固定为 0.2.2，gpui-component 固定为与其匹配的 0.5.1；两者均为 Apache-2.0，组件库承担多行输入、IME、选择与光标行为。ACP 2.2.0 和 rmcp 3.4.0 使用官方 Rust SDK，同为 Apache-2.0。具体传递依赖由 `Cargo.lock` 固定。桌面目标为 Windows、Linux 和 macOS，各平台仍需分别运行验证。

```sh
cargo test -p astra-vn-editor
cargo test --manifest-path Editor/Cargo.toml -p astra-editor
cargo clippy --manifest-path Editor/Cargo.toml -p astra-editor --all-targets -- -D warnings
```

编译或协议测试通过不表示真实 GPU 预览或外部模型 Agent 已验收。当前阶段状态由主仓实施计划统一维护。
