# AstraEditor 实现入口

Editor 使用独立 GPUI workspace。当前边界见[重构契约](../../contracts/rebuild.md)，目标与未完成项见[Editor 模块](../../modules/editor.md)，结构与所有权见[Shell](shell.md)。

操作、Agent 配置、资源导入和 typed Player 预览统一维护在[Editor 手册](../../../Editor/README.md)，不再分别维护 Qt Graph、QML TextArea 或内置模型请求循环蓝图。

## 共用源码与事务

文本、Details、Graph/Timeline、ACP 和 MCP 使用同一个 `AuthoringWorkspace`。Graph 是 source ID 与实际路由的投影；Timeline 关键帧及 easing 编辑生成同源码的版本化 batch。撤销、过期版本和项目切换按同一所有权处理，不创建每个面板独立的剧情或演出数据。

Player 在独立 GPU 窗口使用真实 VN 会话，Editor 负责构建、协议、停止和回收。UE 易用性是目标，不是完成声明；节点布局拖动、曲线、二维场景操作及桌面交互以手册和实施状态中的实际结果为准。

## 检查

```sh
python Tools/check_docs.py
cargo test -p astra-vn-editor
cargo test --manifest-path Editor/Cargo.toml -p astra-editor
cargo clippy --manifest-path Editor/Cargo.toml -p astra-editor --all-targets -- -D warnings
```

项目解析和协议检查不能代替实际工作区、设备声音与 Player 画面验收。
