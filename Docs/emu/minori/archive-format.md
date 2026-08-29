# Minori Archive Format

Minori 游戏以多个 `.paz` archive 分区保存资源。`夏空のペルセウス` 使用：

| Archive | 观测大小 | 预期内容 |
| --- | ---: | --- |
| `bg.paz` + A–J | 3347405076 | 背景资源；主包加十个连续分卷 |
| `bgm.paz` | 285294348 | BGM |
| `scr.paz` | 1914452 | 脚本 `.sc`、流程和文本引用 |
| `st.paz` | 852999948 | 背景、立绘和事件图 |
| `sys.paz` | 32418204 | UI、字体、系统图 |
| `se.paz` | 13554180 | SE |
| `voice.paz` | 326048276 | voice |
| `mov.paz` | 882835526 | movie |

## 解析模型

PAZ reader 采用三段式：

1. 读取 header，识别 archive 类型、entry 数、TOC offset 和 TOC size。
2. mount 时通过有界私有文件接口读取严格 `key.toml`，解开 TOC 后立即释放 index 明文。
3. entry 读取按请求范围建立 source、decrypt 和可选 zlib 流，不落盘明文。

## 证据分层

| 结论 | 来源 | 状态 |
| --- | --- | --- |
| v1+ index size 位于 `0x20`，经 scheme XOR 后必须 8 字节对齐；index 使用 Blowfish | GARbro `ArcPAZ` contract | 已实现，并由八个真实 index 复核 |
| entry descriptor 含 name、offset、unpacked/stored/aligned size 和 packed flag | GARbro contract | 已实现并做 bounds/duplicate 检查 |
| v1/v2 使用 CP932 派生 entry key，v2 按 CRC32 派生 RC4 skip | GARbro contract | family-owned `MinoriPazDecryptor` 已实现 |
| packed entry 解密后执行 zlib | GARbro contract | 已实现 |
| `.pazA` 至 `.pazZ` 是连续逻辑分卷 | GARbro contract | 已实现；空分卷和后缀缺口阻断 |
| Blowfish block 由两个 little-endian `u32` word 组成 | GARbro contract + 本地样本 | 已实现，并由八个真实 index 复核 |
| `mov` entry 不要求 8 字节对齐；movie 分支使用独立 transform | GARbro contract + 本地样本 | 已实现，5 个真实 descriptor 通过 |
| packed entry 解压结果可能比 index 声明值多出不超过 16 字节的全零尾部 | GARbro reader contract + 本地样本 | 顺序流在声明 EOF 后继续验证真实 EOF；仅裁剪全零尾部，非零或第 17 字节阻断 |
| 当前样本八包可完成 mount preflight | 本地样本 | 已成立，共 14502 个 entry；`bg` 的 11 卷连续读取通过 |
| 当前样本八包 decoded full verify | 本地样本 | key-file/streaming identity 已通过：14502 entries、6624958365 decoded bytes |

mount 使用 `minori:/<role>/<entry>`。source hash 通过有界顺序流计算；跨分卷 entry 保持同一逻辑范围。绝对路径、`..`、重复 URI/entry id、短读、越界、未对齐 block、未知 version、源文件 metadata/hash 变化都返回稳定 diagnostic。

`key.toml` 只承载随 title 变化的 key 和可选 type password。下面的 `<hex>` 需要替换为实际值，不能原样使用；八个 role 必须全部出现，`mov.data_key_hex` 必须为空：

```toml
schema = "astra.emu.minori.keys.v1"

[type_passwords]
png = "<optional-cp932-text>"
ogg = "<optional-cp932-text>"
sc = "<optional-cp932-text>"
avi = "<optional-cp932-text>"

[archive_keys.bg]
index_key_hex = "<hex>"
data_key_hex = "<hex>"
[archive_keys.bgm]
index_key_hex = "<hex>"
data_key_hex = "<hex>"
[archive_keys.scr]
index_key_hex = "<hex>"
data_key_hex = "<hex>"
[archive_keys.st]
index_key_hex = "<hex>"
data_key_hex = "<hex>"
[archive_keys.sys]
index_key_hex = "<hex>"
data_key_hex = "<hex>"
[archive_keys.se]
index_key_hex = "<hex>"
data_key_hex = "<hex>"
[archive_keys.voice]
index_key_hex = "<hex>"
data_key_hex = "<hex>"
[archive_keys.mov]
index_key_hex = "<hex>"
data_key_hex = ""
```

## 流式读取

未压缩 entry 只读取覆盖请求范围的对齐加密块；packed entry 每次从起点建立 decrypt + zlib 流并丢弃 offset 前明文。`open_stream` 保留单个顺序状态，multipart source 在加密层跨卷读取。重复 range read 会重新访问加密 source，不产生明文 cache、seek index 或临时文件。

## Lookup

Core 按 archive role 建立 VFS：

```text
script -> scr.paz
background -> bg.paz + volumes
bgm -> bgm.paz
stage/image -> st.paz
system -> sys.paz
se -> se.paz
voice -> voice.paz
movie -> mov.paz
```

查找必须大小写不敏感，但 trace 保留原始 entry name。多个 archive 命中时直接阻断，不能通过 patch overlay 或输入顺序选择结果。

## 安全规则

PAZ key 不写入源码、文档、日志或 report。缺失、越界、schema 不符或 key 非法时 mount 直接阻断；工具不能从 GARbro、exe、补丁 DLL 或 hook 材料自动提取。
