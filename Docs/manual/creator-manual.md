# Creator Manual

创作者先在 Project Wizard 选择 gameplay runtime provider。当前可用模板由 `NativeVnRuntimeProvider` 提供；AstraEMU 和 AstraRPG 仍是 planned provider，只显示不可用诊断和接入边界。选择 NativeVN 后，创作者导入资产，编写 `.astra`，选择 Luau 策略包，用 Graph/Timeline 调整演出，在 PIE 中调试，最后 Cook、Package、Release Gate。需要扩展项目时，通过 Plugin Manager 启用插件并处理依赖诊断。

## 最短流程

1. 选择 `NativeVnRuntimeProvider` template；planned AstraEMU/AstraRPG template 不能创建可发布项目。
2. 导入背景、角色、语音、BGM、字体和 filter profile。
3. 编写 `.astra` Story 与独立 `Ui` source role，并给生产命令和 UI 节点添加稳定 `#@id`。UI 使用 `ui_view`/`ui_bind` 声明 Astra 自有语义，不能直接调用 Yakui API。
4. 选择官方策略包或第三方策略包，在 Plugin Manager 确认 load phase、依赖、权限、命令 provider、Inspector 控件和 Timeline track 都能预览。
5. 用 Graph/Timeline 查看同一 story IR，不创建第二套运行逻辑。
6. 在 PIE 中跑 full-flow YAML scenario。
7. 通过 Package/Release Gate 面板生成二进制 package。

Cook 会读取每个 `AssetSidecar.dependencies` 构建依赖图，并把内容缓存写入项目内 ignored `.astra-cache/cook/`。正常的重复 Cook 应在 `astra.cook_manifest.v2.asset_cook` 中出现 cache hit；cache corruption、依赖缺失/cycle、source hash 或 processor version drift 会直接失败。输出先写 sibling staging directory，全部成功后替换目标目录；失败或按 Ctrl+C 取消时保留上一份完整 Cook 结果。不要手工复制 cache artifact 到 package，也不要把 `.astra-cache/` 提交到仓库。

脚本机制、Luau 策略和可视化规则见 [AstraVN Script Spec](../modules/astra-vn-script.md)。演出模型、标准命令和系统 UI 见 [AstraVN Presentation Model](../modules/astra-vn-presentation-model.md)、[AstraVN Standard Command Library](../modules/astra-vn-standard-commands.md) 和 [AstraVN System UI Profile](../modules/astra-vn-system-ui-profile.md)。样例见 [AstraVN Script Sample](../samples/astra-vn-script/README.md) 和 [AstraVN Advanced Presentation Sample](../samples/astra-vn-advanced/README.md)。

页面开发使用 `.astra` View、Rust schema-bound ViewModel、Luau Controller 和 backend-neutral Theme。当前正式预览和矩阵由 NativeVN cooked package 与 Headless formal runner 驱动，必须走真实 Yakui/AstraText/Scene2D；静态图片或固定矩形不能作为产品 evidence。

## AI

项目授权后，AI 可以直写 `.astra`、Luau 策略和 Graph/Timeline 派生层。每次写入都必须生成 audit event、patch 或 graph diff、undo checkpoint 和 release check。

AI Control 用来绑定 OpenAI、Ollama、ComfyUI 和 ONNX Runtime provider profile，查看 MCP session、Context Pack、Review Queue 和本地加密 trace。Memory Inspector 用来管理角色 `Canon`、`Episodic`、`Player` memory 的 namespace、读写范围、压缩归档和玩家同意状态。ComfyUI 生成的图片、视频或音频先进入 draft sidecar，接受后才进入 Cook，并在 package 阶段写入 `asset.vfs_manifest` 和 `asset.catalog`。

需要把本地模型随 Shipping 包发布时，创作者通过 Import/Cook 导入 ModelBundle。模型权重、tokenizer、reduced runtime、custom op sidecar 和 pipeline config 会成为 Asset VFS content entry；项目不能把模型 payload 放进通用 `package_sections`。Package/Release Gate 会检查 license/provenance、redistribution、voice authorization、模型 fingerprint、VFS mount、section encryption、目标平台 EP 和真实运行报告。

## Reference

| 任务 | 入口 | 验收 |
| --- | --- | --- |
| 创建项目 | Project Wizard | 生成 `project.yaml`、source tree、runtime provider binding、`RuntimeEditorMetadata` 和 policy binding |
| 导入资产 | Content Browser / Import Wizard | sidecar、import audit、cook artifact 都可追踪 |
| 编辑脚本 | Script Editor + Graph/Timeline | source map identity 保持一致 |
| 管理插件 | Plugin Manager / Project Settings | dependency graph、enablement、extension point、diagnostic jump 可解释 |
| 管理 AI | AI Control / Memory Inspector | provider profile、ModelBundle、Context Pack、memory policy、Review Queue 和 consent 可解释 |
| 调试 | PIE + Debugger | diagnostic 能跳到 source_ref |
| 发布 | Package / Release Gate | `astra.release_report.v1` pass |
