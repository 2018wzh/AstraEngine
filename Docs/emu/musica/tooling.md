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

## 从核心槽导出 eden 存档

Family 启动配置 `eden_import_file` 指向游戏副本内的只读原版存档，`eden_import_edition` 与 `script_encoding` 必须匹配文件。原版 eden 的逻辑画布须显式配置为 `logical_width=1024`、`logical_height=640`；`render_width`、`render_height` 独立控制 GPU 光栅尺寸，继续使用等比例黑边与指针映射。核心自有槽保存逻辑画布身份，尺寸不符时拒绝加载。使用 direct 模式进入恢复的消息后，正常推进并通过 F5 或保存页产生核心自有槽，关闭会话后执行：

```sh
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- export-eden-save \
  --game-dir private-copy/game --profile musica.profile.json --slot 10 \
  --output private-export/eden0020.sav
```

输出父目录须已存在，目标 `.sav` 与同名 `.png` 文件均须不存在。PNG 从核心槽实际保存的画面生成，SAV 最后发布；发布失败报错并保留已生成的独立 PNG，不覆盖任何已有文件。CLI 校验游戏身份、脚本、消息历史、当前场景和音频快照后才发布文件；已有文件、损坏槽或不可表达状态均拒绝，读取失败不修改输入。导出不复用原版未知字段。已完成一个真实静态消息片段的原版保存、Family 导入继续、核心保存、成对导出、原版读取继续；范围和剩余限制见 [实施清单](implementation-checklist.md)。
