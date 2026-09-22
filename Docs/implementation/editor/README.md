# AstraEditor 实现设计

当前 Editor 使用 GPUI 与 gpui-component，独立 Cargo workspace 位于 `Editor/`。人工创作是主流程，外部 ACP Agent 和 MCP 共用版本化文档事务。当前边界以 [rebuild contract](../../contracts/rebuild.md)、[模块设计](../../modules/editor.md) 和 [shell](shell.md) 为准。

UE Editor 是易用性目标。已接入项目源文件、资源搜索、Outliner 与 Details 共享选择、属性批量编辑、布局恢复、保存和撤销。Graph 与 Timeline 当前只有源命令投影，完整节点连线、关键帧拖动、曲线和二维场景操作仍需实现，不能据此宣称达到 UE 的创作体验。

## 阅读顺序

| 文档 | 内容 |
| --- | --- |
| [shell.md](shell.md) | 当前 GPUI 工作区、文档事务与进程预览 |
| [graph.md](graph.md) | Graph/Timeline 设计意图；旧 Qt 实现建议不再适用 |
| [script-editor.md](script-editor.md) | 源码编辑设计意图；实际输入组件见 shell |
| [ai-copilot.md](ai-copilot.md) | 历史辅助创作设计；当前采用外部 ACP/MCP，不内置模型循环 |
| [Editor 手册](../../../Editor/README.md) | 构建、启动、项目与 Agent 配置 |

## 检查

```sh
python Tools/check_docs.py
cargo test -p astra-vn-editor
cargo test --manifest-path Editor/Cargo.toml -p astra-editor --lib
cargo clippy --manifest-path Editor/Cargo.toml --all-targets -- -D warnings
```

项目解析和编译可通过 `astra-editor project.yaml --check-project` 检查；它不启动窗口，不能代替实际工作区与 Player 交互验收。
