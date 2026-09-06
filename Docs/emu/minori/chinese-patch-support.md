# Minori 汉化补丁支持设计

状态：设计草案（design-only，未实现）。本文基于两个本地私有商业样本的补丁形态调查，
只记录结构级事实与扩展设计；补丁 payload、密钥与译文不入仓库。样本目录仅存在于
本地私有游戏目录，引用一律用角色名与结构描述。

## 样本事实

### 夏空のペルセウス（Perseus，PAZ v2 引擎）

- 汉化补丁由两个文件构成：补丁版引擎可执行文件与 `perseus_chs.mys`（约 2 MB）。
- `perseus_chs.mys` 头部与 PAZ magic 不同（前 8 字节非 PAZ 特征），是引擎私有容器，
  由补丁版可执行文件在启动时挂载。容器内部格式尚未审计。
- 补丁不覆盖任何原版 PAZ；原文八包保持完整。译文与可能的补丁字体在 `.mys` 内。

### ソレヨリノ前奏詩（Yorino，PAZ v2 引擎 + 补丁 DLL）

- 汉化补丁采用"覆盖档案"形态：`cnbg.paz`、`cnscr.paz`、`cnsys.paz` 三个新增
  PAZ，与原版 `bg/scr/sys.paz` 同格式。`cnscr.paz` 与 `scr.paz` 的加密头部
  前 64 字节逐字节一致，说明同一密钥、同一索引加密与同一 PAZ 版本。
- `cn*.paz` 按文件名覆盖原版同 URI 条目；原版档案保留且不修改。
- 引擎侧改动由 `MinoriPatch.dll` + 补丁版可执行文件承担（推断为修改引擎的
  archive 挂载表与文本代码页路径）；runtime 复刻不需要模拟 DLL，只需达到
  等价数据面：挂载覆盖档案 + 切换文本解码。
- 另有 `Win10汉化文字乱码修复补丁`（rar）：原版引擎按系统 ANSI 代码页转码，
  Win10 下 GBK 转换与旧系统不一致导致乱码。AstraEMU 的文本解码在 family
  字节边界显式完成，不依赖系统代码页，该问题类不会复现，但补丁中"以 GBK
  解释文本"的语义需要由 NLS 显式表达。
- 附加容器 `name.acr`、`AsyncScript.acr` 与 `序章` 内容使用 `.acr` 扩展名，
  头部为明文结构（计数/偏移/hash 表），是引擎另一代容器，格式未审计。
- 游戏目录与补丁目录内**没有 `key.toml`**：八角色的解密密钥来源与 Perseus
  不同（推断内嵌于引擎或补丁 DLL，需独立逆向确认）。

## 现有机制的差距清单

| # | 差距 | 位置 | 现状 |
| --- | --- | --- | --- |
| 1 | 文本 NLS 只有 CP932 | `locale.rs` | `MinoriNls` 已预留 `gbk`/`utf8` 变体，挂载即硬错误；`MinoriLocaleHook` 只实现 `astra.emu.minori.locale.ja-jp.cp932.v1` |
| 2 | hook 调用点硬编码 | `script.rs:208/364`、`provider.rs:5458/11307` | `.sc` 解析、disassemble、资源目标解码直接调用 `MinoriLocaleHook::japanese_cp932()`，不从 profile 传递 |
| 3 | 覆盖档案无法挂载 | `paz.rs` `validate_role_set` | 角色集刚性：必须恰好八个必需角色、文件名必须 `{role}.paz`、去重；`cnscr.paz` 两条都不过 |
| 4 | 条目名解码跟随 hook | `paz.rs` `parse_archive_index` | entry 名已通过 locale hook 解码，GBK 档案的中文文件名需要 GBK hook 到达挂载层 |
| 5 | 字体只有日文字形 | `text_surface.rs` | `FONT_FAMILY = "Noto Sans JP"`，且 identity 校验强制恰好一个字体资产；简体字形（如"画/译/门"等与 JIS 字形集的差异字）需要 CJK 字体绑定 |
| 6 | 密钥来源单一 | `factory.rs` `mount` | 只有 `key.toml`（UTF-8 TOML、八角色 scheme）；Yorino 无此文件 |
| 7 | 容器格式固定 PAZ | `paz.rs` | `.mys` / `.acr` 无法挂载，需要独立格式审计 |

现有可复用基础：VFS resolve 已有 layer/eligibility 与前缀冲突契约（不允许
`BTreeMap` 静默覆盖）；launch profile 的 `content_variant`/`locale_hook`/
`nls` 选项已经把"语言变体"建模为显式绑定；`MinoriPazDecryptor::new_with_locale`
已经在挂载层接受 locale hook。

## 设计

原则：汉化补丁是**数据面变体**，不是新 family，也不是翻译服务。文本解码、
档案覆盖、字体绑定全部走显式 profile 绑定，缺件即 blocking，不回退、不猜测。
正文、密钥与补丁 payload 不进日志、report 或仓库（既有契约继续适用）。

### P1 激活 NLS GBK / UTF-8

- `MinoriLocaleHook` 从单实现改为按 NLS 变体构造：新增
  `astra.emu.minori.locale.zh-hans.gbk.v1` 与 `astra.emu.minori.locale.zh-hans.utf8.v1`
  hook id；`MinoriNls::is_currently_supported` 相应放开。
- 解码失败语义与 CP932 一致：`decode_without_replacement`，坏字节按
  `ASTRA_EMU_MINORI_LOCALE_DECODE` 阻断，不做 replacement 猜测。
- 差距 #2 的四个硬编码调用点改为由 launch profile 的 `locale_hook` +
  `nls` 选项构造并贯穿传递；`content_variant` 引入补丁变体 id（例如
  `soreyori-no-prequel.patched-zh-hans`），variant 与 NLS 组合在 profile
  schema 层校验（原版 variant 不允许 GBK）。
- GBK 引入一个 CP932 不存在的判定边界：GBK 双字节第二字节可以为
  `0x5C`。`.sc` 解析中依赖 `\` 转义与字节偏移的逻辑必须以"解码后的字符
  流 + 原始字节 span"双轨保持（现有 span/offset 结构已按字节记录，需回归
  验证 GBK 文本上 span 不漂移）。

### P2 覆盖档案挂载

- mount options 在 `archive_roles` 之外新增显式 `overlay_roles` 映射：
  `scr: ["scr.paz", "cnscr.paz"]`。每个角色一个**有序**档案列表，顺序即
  优先级，后列覆盖前列；不提供隐式 `cn*` 前缀猜测。
- `MinoriMountedVfs` 为每个角色建立两层 entry 视图：resolve 同 URI 时命中
  覆盖层条目，基础层条目保留可达（诊断用）；两层都不允许静默丢弃。
- 挂载校验：覆盖档案必须与基础档案同 key scheme、同 PAZ 版本、同
  index XOR；entry 名解码使用同一 NLS hook；覆盖档案允许缺少基础档案的
  部分 entry（覆盖是子集），但基础档案必须完整存在。
- full verify / extract / viewer 语义：verify 报告同时覆盖两层；extract
  输出的 URI 需标注来源层（诊断字段，不进公开 report）。

### P3 CJK 字体绑定

- 文本 surface 的字体 profile 从"单一 Noto Sans JP 硬编码"扩展为按
  content variant/NLS 选择的显式字体集合；identity 校验从"恰好一个字体"
  放宽为"恰好等于 profile 声明的集合"，仍然 fail-closed。
- 默认集合：日文变体维持 `Noto Sans JP`；简体变体绑定 `Noto Sans CJK SC`
  （OFL 许可，可入库）或补丁自带字体资产（本地私有，不入仓库）。
- 渲染时对当前消息文本做字形覆盖检查（fontdb 的 coverage 查询），缺字形
  产生有界 diagnostic 并阻断提交，不允许豆腐块静默上屏。

### P4 密钥来源扩展（Yorino 前置）：GARbro scheme 导入

- 实证：GARbro 仓库本体不含 key 值。Minori（引擎名 Musica）的 PAZ 密钥存于
  社区分发的 scheme 数据库：`GARbroDB` 魔数 + zlib + .NET BinaryFormatter
  序列化的 `SchemeDataBase`，其中 `SchemeMap["Musica"]` 携带
  `MusicaScheme{KnownSchemes: signature→PazScheme{Version, ArcKeys:
  档案名→PazKey{IndexKey, DataKey}, TypeKeys: png/ogg/sc/avi 类型密码},
  KnownTitles: 标题→scheme}`。格式逻辑在 `ArcFormats/Musica/ArcPAZ.cs`。
- 本仓已有实现存量：硬化版两阶段 NRBF reader（634 行）与 CMVS importer
  （465 行，`GARbroDB` 解包 → NRBF 图遍历 → 类型化私有 profile 落盘）
  完整存活于 `codex/astraemu-cmvs-runtime` 分支的
  `FamilySupport/astra-emu-garbro-nrbf` 与 `astra-emu-cmvs-cli/src/importer.rs`，
  本分支的 `garbro_nrbf` 空目录是移植时的残留目录。P4 = 移植该 crate 到
  FamilySupport，并按 `astra-emu-cmvs/src/scheme.rs` 的模式写 Minori 版
  importer。
- importer 形态：`astra-emu-minori-cli import-garbro-scheme --scheme-db
  <db> --title <title> --game-dir <root>`，把 `ArcKeys` 按档案名映射为
  八角色 scheme、`TypeKeys` 并入角色 scheme（现有 `MinoriPazDecryptor`
  的 entry RC4 key 推导已经对齐 GARbro 语义），输出与 `key.toml` 同构的
  本地私有密钥文件，并生成 launch/mount profile 引用它。若目标游戏同时
  存在 `key.toml`，两者一致性校验失败即阻断。
- 仓库不提交 key 值与 scheme 数据库；导入产物与 `.astraemu-local` 同级，
  属于本地私有目录。Charter 的"纯 Rust 两阶段 NRBF reader、禁 .NET
  BinaryFormatter"边界由移植的 reader 自身满足（含前向引用两阶段解析、
  节点/深度/解压预算）。

### P5 私有容器（.mys / .acr）：Luau 驱动的索引 Hook

运行时载体已存在：`family-support/src/private_profile.rs` 提供沙箱化
mlua VM（仅 TABLE/STRING/BUFFER 标准库、禁 io/os/load/debug/package、
8 MiB 内存 + 100 万指令 + 2 秒墙钟预算），当前暴露
`astra.family.register_private_profile{id, schema, payload}`，CMVS 用它
在挂载时携带私有 scheme（data-only）。扩展设计在此通道上分两级：

- **数据级（现有形态，.acr 首选）**：`.acr` 头部为明文结构（计数/偏移/
  hash 表），格式审计后在 Rust 实现 reader；Luau 仅作为私有参数载体
  （与 CMVS 相同），不参与解析。适合结构稳定、无自定义字节变换的容器。
- **索引级（新扩展，.mys / 未知变体）**：新增
  `astra.family.register_container_hook{id, schema, code}`，`code` 是一个
  Luau 函数 `parse(header: buffer) → entries`，在挂载时执行**一次**，输入
  为有界的容器头部字节（buffer），输出为有界的 entry 描述表（名称以原始
  字节传递，解码仍由 Rust 的 NLS hook 完成）。Rust 侧在调用前校验容器
  magic 与 hook/schema id，调用后校验 entry 表预算（条目数、总长度、
  偏移单调性），再按表构建 VFS 视图。
- **数据路径不进 Luau**：逐 entry 的数据解密/解压固定走 Rust 原语
  （Blowfish/RC4/XOR/zlib 或审计后新增的纯 Rust 原语），与 PAZ 路径的
  fail-closed 审计口径一致；hook 结果按（容器 hash, hook hash）缓存，
  不逐 entry 重复执行。整容器数据解密交给 Luau 的方案被拒绝：性能
  （百 MB 级数据过 VM）、审计性与既有宪章红线都不允许。
- 沙箱预算沿用现有四项（patch 字节、内存、指令、墙钟），外加输出表
  预算；hook 执行失败、超预算、输出形状不符一律 blocking，无 fallback。

### P6 补丁脚本兼容性验证

- `cnscr.paz` 内的 `.sc` 必须先跑脱敏 census（opcode 计数、`select`
  arity、label 闭合），确认与原版脚本命令集一致；任何新 opcode/operand
  形态按既有 fail-closed 规则阻断，不猜测。
- 汉化组补丁常伴随引擎补丁（寻址/命令变化），census 是发现此类差异的
  第一道闸。

## 实施顺序与验收

P1 → P2 → P3 用 Yorino 样本闭环；P4、P5 按逆向进度插入；P6 在 P2 落地后
立即执行。

E2 验收形态与日文战役相同：挂载覆盖档案 + GBK NLS + CJK 字体的签名
Release 构建，以物理输入完成至少一条中文路线至 Terminal，checkpoint 画面
经人工查看（中文 glyph 无豆腐、消息层/名字/正文完整），`gallery_unlock_count`
断言继续有效。同一构建、profile、输入 identity 下的 run report 与完整
WAV 照常产出。

明确不做：不模拟 `MinoriPatch.dll` 的引擎内 hook；不提供运行时翻译服务或
翻译 overlay/cache；不把补丁 payload、译文或 key 写入 package、report 或
仓库。
