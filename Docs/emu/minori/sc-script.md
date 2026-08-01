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

## Choice

Choice command 的 parser 目前只输出一个保真的候选组：

```text
ChoiceGroup
  options[]
    text
    target_field
```

已确认每项以首个 `:` 分成显示文本与目标字段，且一组严格限制为一至四项。原程序会为该组建立独立 UI 和确认事件；确认结果如何写回 VM、是否跳转，以及右侧字段是否为 label，仍未完成数据流验证。因此当前 runtime 在遇到 `select` 时返回 blocking diagnostic，不接受输入，也不把该字段猜成 `target_label`、条件或变量写入。

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
