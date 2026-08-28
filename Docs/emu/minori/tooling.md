# Minori Tooling

当前 cache-enabled full verify 已连续执行两轮，覆盖 8 个 source、14,502 个 entry、43,818 次 range read 和 6,624,958,365 个 decoded bytes；首轮 `cache_hit_count=29,648`，第二轮 `43,594`，aggregate hash 保持一致。该轮只记录脱敏计数与聚合 hash；identity 漂移、淘汰和损坏恢复仍是独立门禁。

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

当根目录的旧 patch 已存在而对应 mount profile 缺失时，可使用 `recover-garbro-profile` 恢复 profile。调用方必须显式提供相对游戏根的 data-only private patch；工具以当前 GARbro scheme 重建私有 payload 后与该 patch 的注册 payload 做精确字节比较。只在 profile 和临时目标都不存在且比较一致时原子写入 profile；不覆盖 patch、不转换旧 decoder callback patch，也不输出 key、payload 或本地路径。

```sh
cargo run -p astra-emu-minori-cli -- scan-archives --game-dir <case-root>
cargo run -p astra-emu-minori-cli -- import-garbro-scheme --formats <Formats.dat> --title <title> --game-dir <case-root>
cargo run -p astra-emu-minori-cli -- census-scripts --game-dir <case-root> --mount-profile <profile.yaml>
cargo run -p astra-emu-minori-cli -- census-media --game-dir <case-root> --mount-profile <profile.yaml>
```

`scan-archives` 递归识别 `.paz` 与 `.pazA` 至 `.pazZ`，阻断 symlink、空文件、重复 role/part 和不连续分卷。输出只包含 role、文件数、字节数和 inventory hash，不写本地路径或 payload。当前样本结果为 8 个逻辑 archive、18 个物理文件、5742470010 bytes，required role set 完整匹配。

`import-garbro-scheme` 使用纯 Rust 两阶段 NRBF reader，只接受预期的 Musica/PAZ graph。它原子生成 data-only `astraemu.patch.luau` 与 `astraemu.minori.mount.yaml`；任一目标或临时文件已存在即阻断，成对提交失败会回滚本次新文件。Luau 只调用 `astra.family.register_private_profile` 注册 opaque key/policy payload，不参与 index 或 entry 解密。key 不进入 YAML、stdout、report 或日志。

Headless 输入固定采用 `astra.user_input_sequence.v1` 的 internally-tagged `event` 形状，例如键盘输入使用 `{"type":"keyboard","state":"pressed",...}`，退出使用 `{"type":"shutdown"}`。旧 externally-tagged 的 `{"Keyboard":...}`、PascalCase button state 与裸 `"Shutdown"` 会以 `ASTRA_EMU_HEADLESS_INPUT_PARSE` 阻断；调用方必须重新序列化同一物理事件，不能让 reader 兼容两种 wire format。

脚本在等待输入时会暴露 host-owned 的 `runtime.awaiting_input` 观测值。它仅由等待所接受的物理输入 mask 聚合哈希，适合输入序列的 `await` 条件；不会输出 await token、脚本位置、商业文本或资源名。一次确认应将 press/release 排在同一 fixed tick，避免 release 在等待已解决后成为未消费 edge。

当前合法样本已通过真实导入、八包 14,502-entry manifest v2 full verify、同一 identity 的 cache second-run，以及 89 脚本的 payload-free census。两轮 full verify 均执行 43,818 次 range read、读取 6,624,958,365 个 decoded bytes，第二轮 `cache_hit_count=43,594`。补丁、key、输入数据库、明文 cache、导出内容和 disassembly 都留在本地私有目录。

`census-scripts` 当前输出 `astra.emu.minori.sc_census.v5`。除总量、opcode、音频和角色聚合外，`scripts` 数组只保留稳定序号、解码大小、源字节 SHA-256、行/命令计数、opcode 计数和 unknown 计数；不写脚本 URI、正文、operand、label 或跳转目标。这样可以在不泄露商业脚本的前提下定位单文件 parser/runtime 覆盖差异。

`census-media` 只检查 `bg`、`bgm`，逐 frame 调用生产 ANI/SQZ adapter，并用 `image` 验证 PNG。报告仅含格式、entry/frame、像素和尺寸聚合计数；不含 URI、文件名或像素。当前样本通过 4665-entry census：2655 PNG、1951 ANI（6723 frames）、9 SQZ（224 frames）、49 Ogg 和 1 个 metadata database。

## 辅助研究脚本

`Tools/AstraEMU/minori_probe.py`、`minori_paz.py` 和 `minori_sc.py` 只用于格式研究，不是生产 VFS 路径。`minori_paz.py` 不内置 key；没有显式 key file 时只做 probe。所有 decode/extract 产物必须写到 ignored 私有目录。

Windows 无音频设备时，`FamilyAudioService` 仅在 `ProviderUnavailable` 下选择 bounded paced `NullAudioLane`，继续使用 Kira mixer 和有界提交路径，并以 `ASTRA_EMU_AUDIO_NULL_DEVICE` warning 标记。这只保证无设备时的软件运行，不代替 physical audio E3 证据。

消息等待的 host key 集合必须覆盖 family 可直接消费的所有 canonical 输入。Minori 的消息 wait 现在同时声明 `enter`、`space`、`escape` 和 `pointer.primary`；这样鼠标确认会先由 Manager 完成同一个 await，再交给 family 处理 Escape 菜单语义，不会留下旧 wait 与新 wait 并存。该契约由 provider 与 Manager 的定向回归共同锁定。

Control/Auto 改变当前消息的推进方式时，family 保持同一个 wait token，只在 `Input` 与 `Time` 两种 modality 之间重绑定。Manager Core 的 `AwaitBinding` 和 Manager host 的 pending condition 必须同时执行这项受限替换；相同 modality、其它 wait 类型或同一个输出批次内重复 token 都继续返回 `ASTRA_EMU_AWAIT_TOKEN_DUPLICATE`。这不是通用重复 token 宽免，也不允许通过丢弃旧等待或额外推进 tick 来规避错误。

系统页输入隔离：GameView 只把舞台内的真实右键按下/释放映射为 `pointer.secondary`，并同时提交有界 stage-space 坐标。Manager 在该打开请求的 fixed tick 不完成 gameplay await；当 blackboard 的 `minori.system_page` 为除 `none` 以外的已知页面时，所有底层消息/计时等待继续保留。关闭页面不伪造新的 await completion，直到下一次合法 gameplay 输入到达。未知 page、重复 page observation 或系统页期间收到不应有的 provider/await result 都必须返回稳定 blocking diagnostic。
