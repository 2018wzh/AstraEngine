# 创作手册

NativeVN 通过 typed `VnSession` 与编译期组合运行，不选择 gameplay runtime provider。旧 VN 使用独立 [AstraEMU 客户端](astraemu-client.md)，不创建 Editor 工程或 `.astrapkg`。RPG/TRPG 与运行时 AI 不在本轮产品范围。

## 创作流程

1. 打开已有 NativeVN `project.yaml` 或 `.astra`。当前 Editor 的启动、项目装载和限制见 [Editor 手册](../../Editor/README.md)，不把尚无实现的 Project Wizard/Plugin Manager 当作已有入口。
2. 修改 `.astra` 剧情与 UI；文本、Graph、Timeline、Details 和 Agent 共用版本化源码、批量编辑与撤销。图和时间线不是第二份运行数据。
3. 按工程声明管理图片、语音、BGM、字体及媒体。Editor 当前图片导入复用 Cook importer；完整多媒体导入不因已有面板而视为完成。
4. 使用独立 Player 预览真实 VN session。预览、暂停、checkpoint、恢复和关闭的配置及已验证范围只在 Editor 手册维护；协议测试不代表画面、听音或桌面交互验收。
5. 使用现有 Cook/Package/Bundle 链生成产物，并检查 [发布报告](../contracts/release-gate.md)；真实输入、媒体、存读档、退出与路线验证仍需实际运行。

Cook 的项目本地 `.astra-cache/cook/` 为 ignored 产物。缓存身份、依赖和 source drift 由实现校验；失败或取消不能以手工复制半成品替代完整产物，不提交缓存或商业资源。

原生剧情可通过 `nativevn.policy_actions` 显式列出 Luau entry，并使用注册的 typed 命令。包内文本授权填写 `asset:/` ID，不填本机路径。handler 返回显式 Complete/Await，存档保存数据而非闭包、协程或 native handle；恢复后的中断请求不自动重发。配置与实际支持范围见 [包内 Luau 注册动作](../contracts/rebuild.md#包内-luau-注册动作) 和 [Luau 实现](../implementation/luau-policy.md)。

## 外部 Agent

Editor 使用 ACP 外部 Agent 与 MCP 编辑接口，不内置 OpenAI/ONNX provider 或模型 API 循环。自主与逐批确认模式使用同一文档版本和事务；取消或切换项目使旧响应失效。连接和权限操作见 [Editor 手册](../../Editor/README.md#外部-agent)。

## 语义参考

- [剧情与脚本](../modules/astra-vn-script.md)
- [演出模型](../modules/astra-vn-presentation-model.md)、[标准命令](../modules/astra-vn-standard-commands.md)
- [系统 UI](../modules/astra-vn-system-ui-profile.md)
- [Script 示例](../samples/astra-vn-script/README.md)、[演出示例](../samples/astra-vn-advanced/README.md)

实现、设备验收和仍未完成的目标以 [实施状态](../status/implementation-plan.md) 为准，不另维护一套面板完成表。
