# Musica 本地工具

`astra-emu-musica-cli` 在独立 Emulator workspace 中运行，提供 PAZ 目录盘点、SC/媒体 census、eden 存档工具和开发者密钥转换。生产运行时与 Manager 只读取 `astra.emu.musica.keys.v2` 的 `key.toml`；GARbro、旧 keys.v1 TOML 和 profile.v1 JSON reader 全部留在 CLI，不再由 Family 或 Manager 提供。转换不创建游戏会话、不初始化 GPU，也不运行 patch。

从仓库根目录运行：

```bash
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- scan-archives --game-dir .tmp/musica-game
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- list-garbro-titles --formats .tmp/Formats.dat
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- import-garbro-scheme --formats .tmp/Formats.dat --title "本地作品名" --index-size-xor 0 --rc4-skip-crc true --output .tmp/private/key.toml
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- import-key-profile --file .tmp/legacy/key.toml --paz-version 2 --index-size-xor 0 --rc4-skip-crc true --output .tmp/private/native/key.toml
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- import-json-profile --file .tmp/legacy/musica.profile.json --output .tmp/private/converted/key.toml
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- census-scripts --game-dir .tmp/musica-game --profile /absolute/private/key.toml
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- census-media --game-dir .tmp/musica-game --profile key.toml
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- check-eden-restore --file .tmp/native/eden0000.sav --game-dir .tmp/musica-game --profile key.toml --edition japanese --encoding shift-jis
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- export-eden-save --game-dir .tmp/musica-game --profile key.toml --slot 0 --output .tmp/native/eden0020.sav
```

示例中的算法数值只展示语法，必须换成来源明确的实际值。GARbro 的 `Version` 是 PAZ 版本的权威来源；已支持的 `PazScheme` 字段没有索引 XOR 和 RC4 CRC 跳过选项，转换时必须显式传入 `--index-size-xor` 与 `--rc4-skip-crc true|false`。keys.v1 缺少三个算法选项，因此 `import-key-profile` 还必须传入 `--paz-version`。不能按标题猜测、尝试其他 key，或补上未确认的选项。

`list-garbro-titles` 使用有界解压和两阶段 NRBF graph，只输出 `GameRes.Formats.Musica.PazScheme` 对应的作品键名 JSON 数组，不写文件或输出 scheme 内容。名称重复、图结构损坏和预算超限均报错；列出某项不表示其版本受支持。将名称原样传给 `--title`，不按本地目录名猜测。

GARbro reader 先收集 object/metadata/library，再解析有符号 object ID 的 forward reference。未知 record、缺失或重复 reference、角色/类型/key/version 不符合约束时失败，不执行 BinaryFormatter、managed helper 或脚本。Formats.dat 压缩输入最多 256 MiB，解压数据严格小于 256 MiB。

`import-key-profile` 只接受已确认的 `astra.emu.musica.keys.v1` 严格 UTF-8 TOML，输入限 64 KiB，拒绝未知字段、缺角色、坏 hex 和不可表达的 CP932 password。`import-json-profile` 只接受有界 1 MiB 的严格 `astra.emu.musica.profile.v1` JSON，拒绝未知字段和重复 map key；保留所有角色 key、类型密码、`archive_xor`、256-byte `video_key`、算法选项及 `texture_overrides`。旧 JSON 未写 `rc4_skip_crc` 时保留 profile.v1 定义的 `true` 语义；keys.v2 的算法字段全部必填。解析错误不带回私有原文。

所有转换输出使用生产 `MusicaKeyFile` DTO 校验并序列化为 canonical keys.v2 TOML，不输出运行时 JSON。`--output` 必须指向已存在私有目录内的新文件，不能覆盖源文件或既有目标；目录由调用方建立，推荐使用游戏目录外的私有配置目录。文件先在目标目录内建立私有临时文件，写入、sync 后禁止覆盖地发布。Unix mode 为 0600；Windows 在写入前设置 owner-only protected DACL。已有输出、发布冲突和权限失败都不替换旧文件。eden 导出也复用 CLI 本地的私有发布模块。

所有挂载归档的 census/eden 操作都通过运行时 `mount_musica` 读取 keys.v2 `key.toml`。`--profile` 保留为配置路径参数：绝对路径可指向游戏外文件，相对路径以 `--game-dir` 为基准并限制在游戏目录内。生产 key 文件最多 1 MiB，归档和纹理资源仍限制在游戏目录内。转换时不要求存在游戏或替换 PNG；运行时挂载再检查实际资源。

游戏源、key、正文和私有转换产物不得提交；stdout 只输出命令定义的标题列表、census/hash/diagnostic，不输出 scheme 内容。纯 Rust 回归测试使用公开 synthetic fixture，覆盖转换、坏输入、禁止覆盖和权限，不需要游戏/GPU/GUI。整合后执行：

```bash
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- import-garbro-scheme --help
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- import-key-profile --help
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- import-json-profile --help
```
