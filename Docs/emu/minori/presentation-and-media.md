# Minori Presentation And Media

## 资源分区

| Role | Archive | Runtime 命令 |
| --- | --- | --- |
| 背景/立绘/事件图 | `st.paz` | `SetBackground`, `ShowSprite`, `MoveSprite` |
| UI/system | `sys.paz` | message window、config、save/load UI |
| SE | `se.paz` | `PlaySe` |
| Voice | `voice.paz` | `PlayVoice` |
| Movie | `mov.paz` 或 loose | `PlayMovie` |

## Layer Model

AstraEMU Minori core 用固定 layer：

```text
background
event
character slots
effects
message window
system overlay
```

每个 layer command 记录资源名、slot、坐标、alpha、transition、duration 和原始 opcode offset。

## Audio

BGM、SE、voice 分离。Voice replay 从 backlog 触发时不能推进脚本 VM；只提交 `AudioCommand::PlayVoiceReplay`。

## Movie

当前样本 `mov.paz` 为空，但 `PlayMovie` command 仍需要支持。若资源缺失，core 返回 recoverable diagnostic，并允许测试 scenario 断言缺失资源路径。
