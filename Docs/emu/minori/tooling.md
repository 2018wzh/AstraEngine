# Minori Tooling

旧 profile/cache identity 下的八包 full verify 只作为迁移历史保留。当前 key-file/streaming identity 已重新完成同等范围的真实 verify；两次 aggregate hash 相同，但证据身份仍分别记录。

通用 VFS 操作统一走 `astra-emu-cli vfs`。CLI 只从显式 `--game-dir` 和严格 YAML launch profile 建立 family mount，不按注册顺序选择 provider，也不保留旧 `astra-emu-cli minori` 入口。

```sh
cargo run -p astra-emu-cli -- vfs --family minori --game-dir <case-root> --launch-profile <profile.yaml> verify
cargo run -p astra-emu-cli -- vfs --family minori --game-dir <case-root> --launch-profile <profile.yaml> list --uri minori:/
cargo run -p astra-emu-cli -- vfs --family minori --game-dir <case-root> --launch-profile <profile.yaml> stat --uri minori:/scr/example.sc
cargo run -p astra-emu-cli -- vfs --family minori --game-dir <case-root> --launch-profile <profile.yaml> read --uri minori:/scr/example.sc --offset 0 --length 4096
cargo run -p astra-emu-cli -- vfs --family minori --game-dir <case-root> --launch-profile <profile.yaml> extract --output <private-output> --prefix minori:/scr/
```

`verify` 对每个 entry 只打开一次顺序 `open_stream`，以 4 MiB 逻辑块完整读取，校验 decoded size、可用 content hash 和 source mutation；随后再用 `read_range` 独立复读首尾最多 4 KiB。这样 packed entry 不会因每个 4 MiB 块重新建立 zlib 流而形成平方级解压开销，同时仍保留随机读取复核。报告只包含 family、source/entry/range/byte 计数与聚合 hash。`read` 默认也只输出 hash 和范围信息；只有显式 `--format hex` 或 `--format text --encoding <encoding>` 才向 stdout 输出最多 64 KiB 内容。`--output` 可原子写出最多 64 MiB 的私有 range。

`extract` 的 `--prefix`、`--glob` 和 `--entry` 互斥；不传 selector 表示整树。写入前检查容量、大小写冲突、既有目标和路径，全部文件写入 staging tree 后才提交。Linux 提供前台只读 `mount --mountpoint <directory>`；Windows 和 macOS 不声明 FUSE。

Minori 专用 inventory 与脚本研究放在独立 CLI。工具不生成 key，不读取 GARbro `Formats.dat`，也不迁移旧 patch/profile：

```sh
cargo run -p astra-emu-minori-cli -- scan-archives --game-dir <case-root>
cargo run -p astra-emu-minori-cli -- census-scripts --game-dir <case-root> --launch-profile <profile.yaml>
cargo run -p astra-emu-minori-cli -- census-media --game-dir <case-root> --launch-profile <profile.yaml>
```

`scan-archives` 递归识别 `.paz` 与 `.pazA` 至 `.pazZ`，阻断 symlink、空文件、重复 role/part 和不连续分卷。输出只包含 role、文件数、字节数和 inventory hash，不写本地路径或 payload。当前样本结果为 8 个逻辑 archive、18 个物理文件、5742470010 bytes，required role set 完整匹配。

Launch profile 使用 `astra.emu.family_launch_profile.v1`。Minori `family_options` 必须声明原版内容变体、严格的 CP932 locale hook、FVP 风格的 `nls` 编码选择、PAZ version、index XOR、八个 archive role 和相对 `key_file`。`nls` 目前接受 `shift_jis`、`gbk` 和 `utf8` 三个显式值；当前已验证的日文原版只允许 `shift_jis` 挂载，另外两个值保留为 profile 预留，选择后会以 `ASTRA_EMU_MINORI_NLS_UNSUPPORTED` 阻断，绝不回退到 CP932。当前只支持日文原版 `natsuzora-no-perseus.original-ja`；`key.toml` 由用户手工维护，严格使用 `astra.emu.minori.keys.v1`；不能通过 CLI 参数传 key，也不能写入 YAML、stdout、report 或日志。locale hook 只负责把原版 CP932 bytes 在日文绑定下严格解码/编码，不做翻译、文本替换或 GBK 回退。观察到的 `perseus_chs.mys` 和本地化 exe 只能留在研究 inventory，不能作为 runtime source。

```yaml
schema: astra.emu.family_launch_profile.v1
profile_id: minori-local
family_id: minori
mount_id: minori-game
prefix: "minori:/"
runtime:
  entry_uri: "minori:/scr/start.sc"
  launch_mode: title
family_options_schema: astra.emu.minori.mount_options.v4
family_options:
  content_variant: natsuzora-no-perseus.original-ja
  locale_hook: astra.emu.minori.locale.ja-jp.cp932.v1
  nls: shift_jis
  paz_version: 2
  index_size_xor: 0
  key_file: key.toml
  archive_roles: [bg, bgm, scr, st, sys, se, voice, mov]
```

旧 `astraemu.minori.mount.yaml` 不再读取。Manager 只读取游戏目录内的 `astraemu.minori.launch.yaml`；CLI 和研究工具通过 `--launch-profile` 显式接收同一 schema。runtime entry 和 `direct`/`title` 启动模式只来自该文件，不扫描第一个脚本，也不读取环境变量覆盖。

Headless 输入固定采用 `astra.user_input_sequence.v1` 的 internally-tagged `event` 形状，例如键盘输入使用 `{"type":"keyboard","state":"pressed",...}`，退出使用 `{"type":"shutdown"}`。旧 externally-tagged 的 `{"Keyboard":...}`、PascalCase button state 与裸 `"Shutdown"` 会以 `ASTRA_EMU_HEADLESS_INPUT_PARSE` 阻断；调用方必须重新序列化同一物理事件，不能让 reader 兼容两种 wire format。

脚本在等待输入时会暴露 host-owned 的 `runtime.awaiting_input` 观测值。它仅由等待所接受的物理输入 mask 聚合哈希，适合输入序列的 `await` 条件；不会输出 await token、脚本位置、商业文本或资源名。一次确认应将 press/release 排在同一 fixed tick，避免 release 在等待已解决后成为未消费 edge。

当前合法样本的 key-file/streaming identity 已完成八包 14,502-entry full verify：43,818 个逻辑读取范围、6,624,958,365 decoded bytes，aggregate hash 为 `sha256:e641854399512fea4182ebc7de845436d37d3eaef0b31d748b41c8bd23f9e64b`。key、导出内容和 disassembly 都留在本地私有目录。

`census-scripts` 当前输出 `astra.emu.minori.sc_census.v5`。除总量、opcode、音频和角色聚合外，`scripts` 数组只保留稳定序号、解码大小、源字节 SHA-256、行/命令计数、opcode 计数和 unknown 计数；不写脚本 URI、正文、operand、label 或跳转目标。这样可以在不泄露商业脚本的前提下定位单文件 parser/runtime 覆盖差异。

`census-media` 只检查 `bg`、`bgm`，逐 frame 调用生产 ANI/SQZ adapter，并用 `image` 验证 PNG。报告仅含格式、entry/frame、像素和尺寸聚合计数；不含 URI、文件名或像素。当前样本通过 4665-entry census：2655 PNG、1951 ANI（6723 frames）、9 SQZ（224 frames）、49 Ogg 和 1 个 metadata database。

当前 key-file/streaming identity 的 `census-scripts` 也已通过：89 个脚本、33728 行、33695 条命令、29 个 opcode，unknown opcode 为 0。`census-media` 同轮确认上述 4665 个 `bg`/`bgm` 条目，并清点 5 个 AVI container；这些都是脱敏 inventory 证据，不代表媒体播放时序或视觉结果已经验收。

## 辅助研究脚本

`Tools/AstraEMU/minori_probe.py`、`minori_paz.py` 和 `minori_sc.py` 只用于格式研究，不是生产 VFS 路径。`minori_paz.py` 不内置 key；没有显式 key file 时只做 probe。所有 decode/extract 产物必须写到 ignored 私有目录。

Windows 无音频设备时，`FamilyAudioService` 仅在 `ProviderUnavailable` 下选择 bounded paced `NullAudioLane`，继续使用 Kira mixer 和有界提交路径，并以 `ASTRA_EMU_AUDIO_NULL_DEVICE` warning 标记。这只保证无设备时的软件运行，不代替 physical audio E3 证据。

消息等待的 host key 集合必须覆盖 family 可直接消费的所有 canonical 输入。Minori 的消息 wait 现在同时声明 `enter`、`space`、`escape` 和 `pointer.primary`；这样鼠标确认会先由 Manager 完成同一个 await，再交给 family 处理 Escape 菜单语义，不会留下旧 wait 与新 wait 并存。该契约由 provider 与 Manager 的定向回归共同锁定。

Control/Auto 改变当前消息的推进方式时，family 保持同一个 wait token，只在 `Input` 与 `Time` 两种 modality 之间重绑定。Manager Core 的 `AwaitBinding` 和 Manager host 的 pending condition 必须同时执行这项受限替换；相同 modality、其它 wait 类型或同一个输出批次内重复 token 都继续返回 `ASTRA_EMU_AWAIT_TOKEN_DUPLICATE`。这不是通用重复 token 宽免，也不允许通过丢弃旧等待或额外推进 tick 来规避错误。

系统页输入隔离：GameView 只把舞台内的真实右键按下/释放映射为 `pointer.secondary`，并同时提交有界 stage-space 坐标。Manager 在该打开请求的 fixed tick 不完成 gameplay await；family 以公共 `astra.emu.system_ui_active=true` observation 声明独占输入，此时 Manager、Release CLI 和 Headless 都保留底层消息/计时等待。关闭页面发布 `false`，只恢复输入所有权，不伪造 await completion。`minori.system_page` 继续用于页面级测试观察，但 Host 不再解析 Minori 页面枚举。非法布尔值、重复 activity observation 或系统页期间收到不应有的 provider/await result 都返回稳定 blocking diagnostic。
