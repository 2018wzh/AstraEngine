# Musica 本地工具

`astra-emu-musica-cli` 在独立 Emulator workspace 中运行，保留 PAZ 目录盘点、SC/媒体 census 和私有 profile 导入。GARbro/native key 导入实现由自有 Musica 的 `profile_import` API 提供，Manager 直接调用同一实现；无需安装 CLI 才能完成产品配置。导入不创建 Engine session，不使用 FamilySupport、旧 FamilyCore、Extension ABI 或 Luau patch。

从仓库根目录运行：

```bash
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- scan-archives --game-dir .tmp/musica-game
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- list-garbro-titles --formats .tmp/Formats.dat
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- import-garbro-scheme --formats .tmp/Formats.dat --title "本地作品名" --output .tmp/private/musica.profile.json
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- census-scripts --game-dir .tmp/musica-game --profile musica.profile.json
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- census-media --game-dir .tmp/musica-game --profile musica.profile.json
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- import-key-profile --file .tmp/musica-game/key.toml --paz-version 2 --index-size-xor 0 --rc4-skip-crc true --output .tmp/private/native.profile.json
```

`list-garbro-titles` 复用导入器的有界解压和 NRBF graph，只输出 `GameRes.Formats.Musica.PazScheme` 对应的作品键名 JSON 数组，不写文件或输出 scheme 内容。名称重复、图结构损坏和预算超限均报错；列出某项不表示其版本已被导入器支持。将选定名称原样传给 `--title`，不按本地目录名猜测。例如现有数据库使用 `Natsuzora no Perseus`，并非日文显示名。`--profile` 的相对路径以 `--game-dir` 为基准，不重复拼接游戏目录。

导入使用仓库原有纯 Rust 两阶段 NRBF reader，先收集 object/metadata/library，再解析有符号 object ID 的 forward reference。未知 record、缺失或重复 reference、角色/类型/key/version 不符合约束时失败，不运行 managed helper，不执行脚本，也不做启发式 fallback。

`--output` 必须指向已存在私有目录内的新文件，不能覆盖旧文件，也不会写入游戏目录。目录由调用方建立；上例三个 key 算法参数只是命令语法示例，必须使用所选游戏原 profile 的明确值。原生 key 导入只接受已确认的 `astra.emu.musica.keys.v1` 严格 UTF-8 TOML，输入限 64 KiB；拒绝未知字段、缺角色、坏 hex 和不可表达的 CP932 password，TOML 错误不带回原文。

输出 schema 为 `astra.emu.musica.profile.v1`，包含 paz_version、index_size_xor、rc4_skip_crc 和各角色私有 key/password。文件先在目标目录内建立私有临时文件，完成写入和 sync 后以禁止覆盖的方式发布。Unix mode 为 0600；Windows 在写入内容前设置 owner-only protected DACL。已有输出、发布冲突和权限失败都不能覆盖旧文件。旧 Luau/YAML 格式不再读写。

profile 最多 1 MiB，可通过 `profile_file` 显式选择游戏外绝对路径；相对路径仍限制在游戏目录内，归档和纹理路径限制不变。Formats.dat 压缩输入最多 256 MiB，解压数据严格小于 256 MiB。游戏源、key、正文和私有 profile 不得提交；stdout 仅输出已有 census/hash/diagnostic，不输出 scheme 内容。

测试使用普通 Rust tests；纯解析、坏输入、禁止覆盖和权限检查无需 GPU/Headless。真实游戏媒体/结局验收仍由产品测试完成。
