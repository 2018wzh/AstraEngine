# Musica 多版本样本支持矩阵与扩展设计

状态：核心差异已实现并有五样本探测数据支撑；ef 分段执行机制为新增设计项。
本文取代 `multi-variant-support.md` 的早期草稿，覆盖五个本地私有样本：
两个 Steam 官方发行（eden*、ef first/latter tale）、一个日文原版
（夏空のペルセウス）、一个汉化组补丁版（ソレヨリノ前奏詩）。样本只存在于
本地私有目录，正文只写结构事实与支持状态，不落密钥值与文本 payload。

## 样本矩阵

| 样本 | 发行形态 | PAZ 布局 | RC4 skip | 文本编码 | 密钥来源 | 当前支持状态 |
| --- | --- | --- | --- | --- | --- | --- |
| 夏空のペルセウス (jp) | 日文原版 | v2：密文签名、XOR 派生 | 有（CRC>>12） | CP932 | `key.toml`（本地提取） | ✅ 完整 E2：四路线+系统项全通 |
| eden* (en steam) | 官方 Steam | v1 布局 + 0x20 明文签名 `eden_en`、XOR=0 | 无 | **GBK**（`.sc` 正文为官方简体） | 游戏 `key.toml`（已验证与参考表逐字节一致） | ✅ 数据面全通：8 源 9,867 entries、2.89GB full verify、census unknown=0 |
| ef - the first tale (en steam) | 官方 Steam | 同 eden（签名 `ef_first_en_AA`） | 无 | CP932 骨架 + **GBK 行混入**（见下） | 参考 key 表转 `key.toml` | ✅ 数据面全通：8 源 11,089 entries、1.96GB full verify |
| ef - the latter tale (en steam) | 官方 Steam | 同上（签名 `ef_latter_en_AA`） | 无 | 同 ef first | 同上 | ✅ full verify（结果同表） |
| ソレヨリノ前奏詩 (patched) | 汉化组补丁 | v2，`cn*.paz` 覆盖档案 | 有 | CP932 + GBK（补丁） | **无 key.toml**，密钥内嵌引擎 | ⏳ 格式同构已确认；待 GARbro/importer 密钥提取（参考表已有该游戏条目） |

关键版本语义（与汉化组参考实现 `FuckGalEngine/Minori/fuckpaz/main.cpp`
的 `GAME_INFO[].version` 完全对齐）：

- `version 2`（夏空）：header 0x20 处 `index_size` 高字节即 XOR key；
  entry RC4 在 KSA 后按 `crc32(key)>>12 & 0xFF` 跳轮。
- `version 1`（eden*、ef 双部）：header 同样在 0x20/0x24，但带明文游戏
  签名且 XOR=0；entry RC4 **无跳轮**。其余（Blowfish(index)→
  Blowfish(data)→RC4(seed)，seed = `lower(name) + " %08X " + unpacked
  + type_password`，compressed 条目无密码）逐字节一致。
- `mov` 角色一律无 data key（256 字节 mov key 在索引内），与既有契约一致。

## 已落地的引擎核心修改

1. `rc4_skip_crc` mount option：显式选择 v2 跳轮语义（夏空 `true`）或
   v1 无跳轮（eden/ef `false`），贯穿快路径与流式路径。
2. `MusicaLocaleHook` 编码枚举：CP932 与 GBK 两个 hook id，严格无替换
   解码；NLS↔hook 绑定校验（不允许错配）。
3. `content_variant` 集合扩展：`eden.original.multi` 已验证；入口点识别
   扩为 `perseus.exe / eden_en.exe / ef_first_AA.exe / ef_latter_en_AA.exe`。
4. 脚本解析贯穿 profile hook：provider `open` 时按 `musica.nls` 记录
   hook，entry/chain/gallery/save-slot/audit 全部统一；census CLI 同样。

## 新增设计项：ef 的 `[j]`/`[e]` 分段执行

ef 的 `.sc` 行带一字符方括号前缀：`[j]` 行仅在日文模式执行、`[e]` 行仅在
英文模式执行、无前缀行两种模式都执行（控制流 if/goto/label 均可带前缀，
形成两条分支路线）。这是引擎层的**语言条件执行**，不是文本标记。支持方案：

- VM 状态增加 `script_language`（由 profile `nls` 派生：cp932→`j`，
  后续 gbk 变体可映射 `c`）；解析器把 `[x]` 前缀解析为行的语言守卫
  （保留原始字节 span）。
- 执行器对带守卫的行：守卫匹配当前语言则执行，否则跳过该行（计入
  executed trace，不产生副作用）；无守卫行不受影响。
- snapshot 记录语言选择；未知前缀字母按 unknown-operand 规则阻断。

### 混合编码行（ef 现实约束）

ef 的无前缀 `.message` 正文存在 GBK 行（中国渠道注入的简体中文），
与 CP932 行混排且无行级标记区分。严格单编码解码必然失败。可选方案
（按优先级）：

a. **运行时双解码回退**：先按 profile NLS 严格解码；失败时按显式
   `fallback_nls`（profile 声明，如 `gbk`）严格解码；两者都失败才
   blocking。两次解码都无替换、有界、结果进 snapshot——不引入猜测式
   检测，回退是显式配置。
b. 行级编码审计：census 输出每文件 CP932/GBK 行计数，发行 profile 按
   审计结果选择主编码，回退仅对少数文件生效。

推荐 (a)，机制最小且 fail-closed 语义不变。

## 与汉化补丁设计的关系

- 前奏诗密钥条目已存在于参考 key 表（`ソレヨリノ前奏詩`），GARbro
  importer（chinese-patch-support.md P4）可直接转出 `key.toml`；
  `cn*.paz` 覆盖档案仍按该文档 P2 的 `overlay_roles` 方案。
- 汉化组补丁的 GBK 文本 + CP932 骨架与 ef 的混合编码是同一问题，
  统一由上述双解码回退覆盖。

## 验收边界

- 已验证（数据面，五样本中四样本全通）：夏空（完整 E2）、eden*
  （full verify + census unknown=0 + GBK 明文）、ef first/latter
  （full verify + 脚本明文）。
- 待实现后验证：ef `[j]`/`[e]` 执行守卫与路线 E2、双解码回退的
  census/E2、前奏诗密钥导入 + 覆盖挂载 + GBK 路线 E2、GBK 渲染字形
  覆盖（Noto Sans CJK SC 绑定）。
- 明确不做：不内嵌任何游戏密钥入仓库；不模拟官方/汉化组 DLL；不做
  无配置的编码猜测。
