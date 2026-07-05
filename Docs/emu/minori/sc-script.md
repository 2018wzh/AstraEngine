# Minori SC Script Notes

## 反编译形态

第一阶段反编译只要求保真，不要求还原作者源码：

```text
00012340: label L_0010
00012358: msg speaker="..." text="..." voice="..."
00012410: bg file="..." transition=fade time=500
00012438: sprite slot=1 file="..." x=640 y=0 alpha=255
00012490: select id=42 choices=[...]
00012520: jump L_0100
```

如果字段无法命名，使用 `op_XX raw=<hex>`，同时把 stack/operand 数量写入 trace。

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
