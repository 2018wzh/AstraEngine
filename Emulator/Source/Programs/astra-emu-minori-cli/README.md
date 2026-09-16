# Minori 本地工具

`astra-emu-minori-cli` 在独立 Emulator workspace 中运行。它保留 PAZ 目录盘点、SC/媒体 census 和 GARbro scheme 导入；使用 Minori 自有 archive API，不创建 Engine session，不使用 FamilySupport、旧 FamilyCore、Extension ABI 或 Luau patch。

从仓库根目录运行：

```bash
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-minori-cli -- scan-archives --game-dir .tmp/minori-game
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-minori-cli -- list-garbro-titles --formats .tmp/Formats.dat
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-minori-cli -- import-garbro-scheme --formats .tmp/Formats.dat --title "本地作品名" --game-dir .tmp/minori-game
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-minori-cli -- census-scripts --game-dir .tmp/minori-game --profile minori.profile.json
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-minori-cli -- census-media --game-dir .tmp/minori-game --profile minori.profile.json
```

`list-garbro-titles` 复用导入器的有界解压和 NRBF graph，只输出 `GameRes.Formats.Musica.PazScheme` 对应的作品键名 JSON 数组，不写文件或输出 scheme 内容。名称重复、图结构损坏和预算超限均报错；列出某项不表示其版本已被导入器支持。将选定名称原样传给 `--title`，不按本地目录名猜测。例如现有数据库使用 `Natsuzora no Perseus`，并非日文显示名。`--profile` 的相对路径以 `--game-dir` 为基准，不重复拼接游戏目录。

导入使用仓库原有纯 Rust 两阶段 NRBF reader，先收集 object/metadata/library，再解析有符号 object ID 的 forward reference。未知 record、缺失或重复 reference、角色/类型/key/version 不符合约束时失败，不运行 managed helper，不执行脚本，也不做启发式 fallback。

输出唯一 `minori.profile.json`，schema 为 `astra.emu.minori.profile.v1`，包含 paz_version、index_size_xor 和各角色私有 key/password。文件先在游戏目录内建立私有临时文件，完成写入和 sync 后以禁止覆盖的方式发布。Unix mode 为 0600；Windows 在写入内容前设置 owner-only protected DACL。已有输出、发布冲突和权限失败都不能覆盖旧文件。旧 Luau/YAML 格式不再读写。

profile 最多 1 MiB，并且必须位于所选游戏目录中。Formats.dat 压缩输入最多 256 MiB，解压数据严格小于 256 MiB。游戏源、key、正文和私有 profile 不得提交；stdout 仅输出已有 census/hash/diagnostic 和无私有内容的导入状态。

测试使用普通 Rust tests；纯解析、坏输入、禁止覆盖和权限检查无需 GPU/Headless。真实游戏媒体/结局验收仍由产品测试完成。
