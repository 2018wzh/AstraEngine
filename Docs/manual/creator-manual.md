# Creator Manual

创作者从 Project Wizard 创建 AstraVN 项目，导入资产，编写 `.astra`，选择 Lua 策略包，用 Graph/Timeline 调整演出，在 PIE 中调试，最后 Cook、Package、Release Gate。

## 最短流程

1. 选择 AstraVN template。
2. 导入背景、角色、语音、BGM、字体和 filter profile。
3. 编写 `.astra` story，给生产命令添加稳定 `#@id`。
4. 选择官方策略包或第三方策略包，确认策略节点、Inspector 控件和 Timeline track 都能预览。
5. 用 Graph/Timeline 查看同一 story IR，不创建第二套运行逻辑。
6. 在 PIE 中跑 full-flow YAML scenario。
7. 通过 Package/Release Gate 面板生成二进制 package。

脚本机制、Lua 策略和可视化规则见 [AstraVN Script Spec](../modules/astra-vn-script.md)。样例见 [AstraVN Script Sample](../samples/astra-vn-script/README.md)。

## AI

项目授权后，AI 可以直写 `.astra`、Lua 策略和 Graph/Timeline 派生层。每次写入都必须生成 audit event、patch 或 graph diff、undo checkpoint 和 release check。
