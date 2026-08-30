# Minori SC Script Notes

## 反编译形态

第一阶段反编译按 CP932 行源码保真，不改写正文：

```text
00012340: .label route_a
00012358: .message <raw operands>
00012410: .stage <raw operands>
00012438: .transition <raw operands>
00012490: .select <raw operands>
00012520: .goto route_b
```

如果 operand 字段无法命名，保留原始 CP932 bytes 和 source span，不输出猜测字段。正文、完整 raw operand 与 disassembly 不进入 report 或日志。

## Message

Message command 至少要恢复：

- 文本正文。
- 说话人或 name window 字符串。
- voice 资源名或 voice id。
- wait-for-input 标记。
- backlog 是否记录。

原程序的文字 parser 还识别正文内控制标记。当前已由 IDA 确认并进入 typed IR 的集合是 `\\a`（自动推进）、`\\v`（等待语音结束）和 `\\x{load,...}`（按延时更新角色层）。控制标记不会进入可见正文或 backlog。`load` 只接受已验证的 4 至 6 个参数、受限 PNG 名称、角色 slot、非负延时、transition 和 opacity；未知命令、截断花括号、额外参数或数值越界直接返回稳定 diagnostic。`MsgSubCmd` dispatcher 中存在但样本没有使用的 `pos/trans/vis` 仍只记作原程序事实，不在 runtime 中猜测 operand。

## Choice

Choice command 的 parser 输出一个保真的候选组，并将已经验证的目标 label 单独保存在 CFG 中：

```text
ChoiceGroup
  options[]
    text
    target_field
```

已确认每项以首个 `:` 分成显示文本与目标字段，且一组严格限制为一至四项。runtime 只接受 parser 已确认的目标 label：进入选择时建立有界 `Choice` wait，呈现层只传递各显示文本的 hash、选中索引和 option 数量；确认后消费一次性选择输入，按选中项跳转到对应 label，并清除 choice presentation。目标字段为空、不是 parser 建立的 label、选项数量超出范围、重复等待或跳转越界都会返回 blocking diagnostic。显示文本不进入 ABI、report 或日志，右侧字段不会被猜测成条件、变量或其他语义。

## 演出命令

图像命令统一投射为 AstraEMU presentation command：

```text
SetBackground(file, transition, duration)
ShowSprite(slot, file, x, y, z, alpha)
MoveSprite(slot, x, y, alpha, duration)
HideSprite(slot, duration)
PlayBgm(file, loop)
PlaySe(file)
PlayVoice(file, character)
PlayMovie(file)
Wait(duration)
WaitInput
```

未识别参数必须保存在 opaque operand 中，避免影响后续复现。
