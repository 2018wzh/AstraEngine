# Minori Tooling

正式入口是 `astra-emu-cli minori`：

```sh
cargo run -p astra-emu-cli -- minori verify --game-dir <case-root> --version 2 --index-size-xor <value>
cargo run -p astra-emu-cli -- minori census-scripts --game-dir <case-root>
cargo run -p astra-emu-cli -- minori extract --game-dir <case-root> --output <private-output> --role scr
cargo run -p astra-emu-cli -- minori import-garbro-scheme --formats <Formats.dat> --title <title> --game-dir <case-root>
```

`verify` 检查六包、index、全部 entry descriptor，并对每个 entry 做首段与尾段随机读取；报告只给出 entry、cache hit/miss 和 range-read 数量。`census-scripts` 直接从 VFS 读取全部 `scr/*.sc`，只输出文件、行、command、operand size 与 token 计数，不输出正文、文件名或 raw operand。`extract` 的 role、glob 和单 URI selector 互斥，写入前检查容量、大小写冲突和既有输出；所有文件先写入同卷 staging tree，全部成功后才以目录 rename 提交，失败时不保留部分输出。

Linux 另有前台只读 `minori mount --mountpoint <directory>`。Windows 和 macOS 不声明 FUSE；三种桌面系统都保留 `verify` 与 `extract`。

`import-garbro-scheme` 使用仓库内纯 Rust 两阶段 NRBF reader。第一阶段收集 object、class metadata、library 和有符号 object id，第二阶段校验 forward reference，再读取预期的 Musica/PAZ graph。未知 record、重复 id、断裂 reference、缺 role、异常 key size、缺 title 或既有补丁都会阻断。命令只在游戏目录写私有 Luau 补丁，终端不打印 key；实现不调用 .NET `BinaryFormatter`、managed helper、外部进程、启发式扫描或隐藏 fallback。

当前合法 `Formats.dat` 已完成真实导入。由该入口生成的补丁通过六包 9837 个 entry 全读验证，同 identity 复核为 9837 cache hits/0 misses。补丁、key、输入数据库和明文 cache 仍只保存在本地私有目录。

## `minori_probe.py`

```bash
python Tools/AstraEMU/minori_probe.py "<minori-case-root>" --json
```

输出 `.paz`、`.mys`、`.exe`、`.chm` 的大小和 magic 标签。

## `minori_paz.py`

```bash
python Tools/AstraEMU/minori_paz.py "<minori-case-root>/scr.paz" --json
python Tools/AstraEMU/minori_paz.py scr.paz --key-file local-key.hex --json
```

该工具不内置 key。没有 `--key-file` 时只输出 probe 信息。

## `minori_sc.py`

```bash
python Tools/AstraEMU/minori_sc.py decoded.sc --json
```

输入必须是已经合法解出的 `.sc` 或同等文本/二进制脚本片段。输出 message/select/voice/bgm/se/image 等 marker。

## 约束

所有 extract/decode 产物只能写到显式输出目录，不提交到仓库。
