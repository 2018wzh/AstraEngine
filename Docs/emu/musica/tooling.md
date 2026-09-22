# Musica 工具状态

Musica 已通过当前 Family API 接入 [独立 Host](../../migrations/astraemu-independent-host.md)。此前的通用 VFS mount、Luau private profile、Headless 和签名 provider 工作流不适用于当前 Host。

`astra-emu-musica-cli` 已加入独立的 `Emulator` workspace，提供 archive inventory、两阶段纯 Rust NRBF reader、GARbro scheme importer、脚本/媒体 census 和 eden 存档检查。CLI 的格式检查不代表 Manager 内的实际游玩通过。

NRBF reader 先收集对象和 metadata，再解析 forward reference；未知 record、断裂 reference、重复 id 和非预期 Musica/PAZ graph 必须报错。后续接入不得改用 BinaryFormatter 或外部 managed helper。旧 importer 输出的 Luau/mount 配置属于旧架构，届时应按新 Family 自有文件边界调整，不保留兼容桥。

## eden 存档检查

```bash
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- inspect-eden-save \
  --file private-copy/eden0001.sav --edition english --encoding gbk
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- check-eden-restore \
  --file private-copy/eden0001.sav --game-dir private-copy/game \
  --profile musica.profile.json --edition english --encoding gbk
```

`profile` 相对游戏目录解析。两个命令都只读输入，不生成存档，也不覆盖原版槽位。第一个命令检查容器；第二个命令读取挂载的真实脚本，检查当前消息、历史消息和 VM 恢复候选。它不会打开 GPU 窗口或播放声音，不能替代原版导入、继续游玩、导出和原版读回的完整流程。

恢复仅接受已明确映射的字段。未知字段、无法恢复的效果、选择记录、历史脚本缺失或消息内容不匹配时返回错误，不丢弃历史或从邻近位置继续。实现边界见 [核心说明](../../../Emulator/Source/Families/astra-emu-musica/README.md)。

## 辅助研究脚本

`Tools/AstraEMU/musica_probe.py`、`musica_paz.py` 和 `musica_sc.py` 仍用于格式研究，不是当前 Host 的生产路径。输入数据库、key、解包内容和 disassembly 保持本地私有。

以往实验结果见 [历史移植记录](porting-log.md)；其中的命令和测试结果不能作为新 Host 的当前状态。
