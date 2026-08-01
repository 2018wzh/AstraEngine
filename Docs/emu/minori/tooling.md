# Minori Tooling

通用 VFS 操作统一走 `astra-emu-cli vfs`。CLI 只从显式 `--game-dir` 和严格 YAML mount profile 建立 family mount，不按注册顺序选择 provider，也不保留旧 `astra-emu-cli minori` 入口。

```sh
cargo run -p astra-emu-cli -- vfs --family minori --game-dir <case-root> --mount-profile <profile.yaml> verify
cargo run -p astra-emu-cli -- vfs --family minori --game-dir <case-root> --mount-profile <profile.yaml> list --uri minori:/
cargo run -p astra-emu-cli -- vfs --family minori --game-dir <case-root> --mount-profile <profile.yaml> stat --uri minori:/scr/example.sc
cargo run -p astra-emu-cli -- vfs --family minori --game-dir <case-root> --mount-profile <profile.yaml> read --uri minori:/scr/example.sc --offset 0 --length 4096
cargo run -p astra-emu-cli -- vfs --family minori --game-dir <case-root> --mount-profile <profile.yaml> extract --output <private-output> --prefix minori:/scr/
```

`verify` 以 4 MiB range 完整流读每个 entry，校验 decoded size、可用 content hash、source mutation，并复读首尾最多 4 KiB。报告只包含 family、source/entry/range/byte/cache 计数与聚合 hash。`read` 默认也只输出 hash 和范围信息；只有显式 `--format hex` 或 `--format text --encoding <encoding>` 才向 stdout 输出最多 64 KiB 内容。`--output` 可原子写出最多 64 MiB 的私有 range。

`extract` 的 `--prefix`、`--glob` 和 `--entry` 互斥；不传 selector 表示整树。写入前检查容量、大小写冲突、既有目标和路径，全部文件写入 staging tree 后才提交。Linux 提供前台只读 `mount --mountpoint <directory>`；Windows 和 macOS 不声明 FUSE。

Minori 专用导入与脚本研究放在独立 CLI：

```sh
cargo run -p astra-emu-minori-cli -- scan-archives --game-dir <case-root>
cargo run -p astra-emu-minori-cli -- import-garbro-scheme --formats <Formats.dat> --title <title> --game-dir <case-root>
cargo run -p astra-emu-minori-cli -- census-scripts --game-dir <case-root> --mount-profile <profile.yaml>
cargo run -p astra-emu-minori-cli -- census-media --game-dir <case-root> --mount-profile <profile.yaml>
```

`scan-archives` 递归识别 `.paz` 与 `.pazA` 至 `.pazZ`，阻断 symlink、空文件、重复 role/part 和不连续分卷。输出只包含 role、文件数、字节数和 inventory hash，不写本地路径或 payload。当前样本结果为 8 个逻辑 archive、18 个物理文件、5742470010 bytes，required role set 完整匹配。

`import-garbro-scheme` 使用纯 Rust 两阶段 NRBF reader，只接受预期的 Musica/PAZ graph。它原子生成 data-only `astraemu.patch.luau` 与 `astraemu.minori.mount.yaml`；任一目标或临时文件已存在即阻断，成对提交失败会回滚本次新文件。Luau 只调用 `astra.family.register_private_profile` 注册 opaque key/policy payload，不参与 index 或 entry 解密。key 不进入 YAML、stdout、report 或日志。

当前合法样本已通过真实导入、八包 14502-entry manifest v2 full verify，以及 89 脚本的 payload-free census。full verify 共执行 43818 次 range read、读取 6624958365 个 decoded bytes；该轮显式关闭 cache。八包 cache identity 复核因平台缓存卷空间不足保持 blocking。补丁、key、输入数据库、明文 cache、导出内容和 disassembly 都留在本地私有目录。

`census-media` 只检查 `bg`、`bgm`，逐 frame 调用生产 ANI/SQZ adapter，并用 `image` 验证 PNG。报告仅含格式、entry/frame、像素和尺寸聚合计数；不含 URI、文件名或像素。当前样本通过 4665-entry census：2655 PNG、1951 ANI（6723 frames）、9 SQZ（224 frames）、49 Ogg 和 1 个 metadata database。

## 辅助研究脚本

`Tools/AstraEMU/minori_probe.py`、`minori_paz.py` 和 `minori_sc.py` 只用于格式研究，不是生产 VFS 路径。`minori_paz.py` 不内置 key；没有显式 key file 时只做 probe。所有 decode/extract 产物必须写到 ignored 私有目录。
