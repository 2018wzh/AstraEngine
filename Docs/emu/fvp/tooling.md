# FVP 工具与调试

当前入口是 [独立 Host 重构](../../migrations/astraemu-independent-host.md)中的 Windows Manager。旧通用 CLI、Headless runner、VFS mount profile、continuation snapshot 和 parity report 工具已删除，不再作为 FVP 的运行或验收入口。

FVP 自行读取原生游戏文件、解码、混音、绘制并管理原生存档。Host 只接收最终帧和 PCM，不为 FVP 建立统一 VFS 或 package。实现状态见 [Stage 5](../../status/stages/stage-5-astra-emu.md)。

## 保留的格式研究脚本

以下脚本仍用于检查文件格式，不启动新 Host，也不代表游戏已经可运行：

```sh
python Tools/AstraEMU/fvp_probe.py <game-root> --json
python Tools/AstraEMU/fvp_hcb.py <game-root>/Game.hcb --json
python Tools/AstraEMU/fvp_bin.py <game-root>/bgm.bin --json
```

RFVP 上游的 disassembler、assembler、hcb2lua_decompiler、lua2hcb 和 nvsg_pack 仍可作为研究参考；这些工具不属于当前 AstraEngine workspace。公开测试使用合成 fixture，商业文件及其解包、反汇编结果只留在本地私有目录。

## 运行检查

新 Host 集成后，检查键盘与鼠标、系统页、语音和 BGM、完整视频、原生存读档、退出冷启动，以及至少一条完整路线。鼠标 move 与 down/up 保留独立边沿和坐标；Ctrl 的效果由 RFVP 原有输入语义决定。FVP 不接入翻译，本轮也不使用模拟 Family 代替翻译联调。
