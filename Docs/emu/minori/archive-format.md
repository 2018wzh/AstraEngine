# Minori Archive Format

Minori 游戏以多个 `.paz` archive 分区保存资源。`夏空のペルセウス` 使用：

| Archive | 观测大小 | 预期内容 |
| --- | ---: | --- |
| `scr.paz` | 1914452 | 脚本 `.sc`、流程和文本引用 |
| `st.paz` | 852999948 | 背景、立绘和事件图 |
| `sys.paz` | 32418204 | UI、字体、系统图 |
| `se.paz` | 13554180 | SE |
| `voice.paz` | 326048276 | voice |
| `mov.paz` | 882835526 | movie |

## 解析模型

PAZ reader 采用三段式：

1. 读取 header，识别 archive 类型、entry 数、TOC offset 和 TOC size。
2. 使用外部 key config 解开 TOC。key 来源只能是命令行、用户配置或 case manifest。
3. 对 entry payload 执行 per-file transform、zlib inflate 或 raw passthrough。

## 证据分层

| 结论 | 来源 | 状态 |
| --- | --- | --- |
| v1+ index size 位于 `0x20`，经 scheme XOR 后必须 8 字节对齐；index 使用 Blowfish | GARbro `ArcPAZ` contract | 已实现，并由六个真实 index 复核 |
| entry descriptor 含 name、offset、unpacked/stored/aligned size 和 packed flag | GARbro contract | 已实现并做 bounds/duplicate 检查 |
| v1/v2 使用 CP932 派生 entry key，v2 按 CRC32 派生 RC4 skip | GARbro contract | 纯 Rust `MinoriPazDecryptProvider` 已实现 |
| packed entry 解密后执行 zlib | GARbro contract | 已实现 |
| `.pazA` 至 `.pazZ` 是连续逻辑分卷 | GARbro contract | 已实现；空分卷和后缀缺口阻断 |
| Blowfish block 由两个 little-endian `u32` word 组成 | GARbro contract + 本地样本 | 已实现，并由六个真实 index 复核 |
| `mov` entry 不要求 8 字节对齐；movie 分支使用独立 transform | GARbro contract + 本地样本 | 已实现，5 个真实 descriptor 通过 |
| packed entry 解压结果可能带不超过 16 字节的全零尾部 | 本地样本观察 | 仅在可证明全零时裁剪；非零或更长尾部阻断 |
| 当前样本六包可完整挂载和读取 | 本地样本 | 已成立，共 9837 个 entry；第二轮 29720 次 range read 全部命中 cache |

mount 使用 `minori:/<role>/<entry>`。绝对路径、`..`、重复 URI/entry id、短读、越界、未对齐 block、未知 version、源文件 metadata/hash 变化都返回稳定 diagnostic。

## Lookup

Core 按 archive role 建立 VFS：

```text
script -> scr.paz
stage/image -> st.paz
system -> sys.paz
se -> se.paz
voice -> voice.paz
movie -> mov.paz
patch -> *.mys / *.acr / 外部只读 mount
```

查找必须大小写不敏感，但 trace 保留原始 entry name。多个 archive 命中时，patch mount 优先，之后按 role 固定顺序。

## 安全规则

PAZ key 不写入源码，不写入文档正文。工具和 core 遇到缺 key 时返回 `NeedsUserKey` diagnostic，不能尝试从 exe、补丁 DLL 或 hook 材料自动提取。
