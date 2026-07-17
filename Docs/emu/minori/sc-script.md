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

Choice command 输出：

```text
ChoiceGroup
  options[]
    text
    condition
    target_label
    variable_write
```

Core 运行时在固定 tick 边界接受 input，选项结果写入 VM state，再继续执行 jump。

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
