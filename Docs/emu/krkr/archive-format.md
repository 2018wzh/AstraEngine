# XP3 Archive Format

XP3 是 KrKr family 的主要资源容器。AstraEMU 第一阶段只需要可靠读取 index、建立虚拟 storage、定位 segment、计算校验和报告能力缺口。payload decode 由格式 provider 或 KrKr compat core 处理。

## Header

样本和 `XP3Viewer` 代码都使用 11 字节 magic：

```text
58 50 33 0D 0A 20 0A 1A 8B 67 01
```

其后是 little-endian `u64 index_offset`。样本中 `data.xp3` 的 `index_offset` 指向文件末尾附近，`voice.xp3` 也一样。读取器不能假设 index 紧跟 header。

## Index Record

index record 有两种样本形态：

| 形态 | 布局 | 样本 |
| --- | --- | --- |
| zlib index | `u8 flag=1`、`u64 original_size`、`u64 archive_size`、zlib data | 大多数 base archive |
| raw index | `u8 flag=0`、`u64 size`、raw `File` chunks | `patch2.xp3`、`patchAI.xp3` |

`XP3Viewer` 的老代码在 raw index 分支会回退 8 字节再读 index data，这和样本中的 raw 布局一致。AstraEMU reader 应同时接受这两种布局，并把未知 `flag & 7` 记录成 format diagnostic。

## File Chunk

index data 是一串 `File` chunk。每个 `File` chunk 内至少有：

| Chunk | 字段 | 用途 |
| --- | --- | --- |
| `info` | flags、original size、archive size、UTF-16LE filename | storage name、大小和加密/过滤标记 |
| `segm` | segment flag、offset、original size、archive size | payload 位置和 zlib/raw segment |
| `adlr` | Adler-32 | 快速校验 |
| `time` | FILETIME | 可选时间戳 |

文件名是 UTF-16LE，不是当前系统 code page。样本里有日文、中文和反斜杠路径，例如 `scenario\start.ks`、`font\ctxfontprefs.tjs`、`バンド001_03月_プロローグ上（現状）.ks.scn`。

## Segment

`segm` 记录的 low 3 bits 是编码方式：

| 值 | 含义 |
| ---: | --- |
| `0` | raw segment |
| `1` | zlib segment |
| 其他 | 不支持，必须诊断 |

3lj 样本的所有已解析条目 segment flag 都是 `1`，也就是 payload segment 压缩。`info.flags` 大多是 `0x80000000`，`patchAI.xp3` 和 `patch2.xp3` 是 `0`。AstraEMU 不能把这个 bit 简化成全局加密开关；它只应进入 archive metadata，并交给具体 family/provider 判断。

## Reader Contract

最小 reader 输出：

```text
ArchiveIndex {
  archive_name,
  index_offset,
  index_encoding,
  entries: [
    {
      storage_name,
      normalized_storage_key,
      info_flags,
      original_size,
      archive_size,
      adler32,
      segments: [{ flag, offset, original_size, archive_size }]
    }
  ]
}
```

`normalized_storage_key` 只用于查找，原始 `storage_name` 必须保留。KrKr storage 名大小写、全角字符和目录分隔符都可能被脚本引用。

## 不做的事

- 不在文档或工具中复制 payload。
- 不实现通用绕过或商业保护处理。
- 不把 DLL filter、hook 或反编译流程写入 public contract。
- 不把 XP3 直接变成 Astra package；compat core 先按 KrKr 语义读取，Cook 到 Astra package 是后续迁移任务。
