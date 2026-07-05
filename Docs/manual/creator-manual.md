# Creator Manual

创作者从 Project Wizard 创建 AstraVN 项目，导入资产，编写 `.astra`，用 Graph/Timeline 调整演出，在 PIE 中调试，最后 Cook、Package、Release Gate。

## 最短流程

1. 选择 AstraVN template。
2. 导入背景、角色、语音、BGM、字体和 filter profile。
3. 编写 `.astra` story，给生产命令添加稳定 `#@id`。
4. 用 Graph/Timeline 查看同一 story IR，不创建第二套运行逻辑。
5. 在 PIE 中跑 full-flow YAML scenario。
6. 通过 Package/Release Gate 面板生成二进制 package。

## AI

Trusted session 可以直写源文件，但每次直写都有授权范围、patch、audit event 和 undo checkpoint。未授权 AI 输出进入 Review Queue。
