# SoftPAL Archive Format

SoftPAL 资源解析分两层：`ARCHIVE.DAT` 负责声明搜索 base path，PAC 负责在每个 base 下按 key 查找条目。`sena-rs` 的 `ResourceManager::bootstrap` 是当前最清楚的参考实现。

## Bootstrap

启动时按以下顺序建立资源目录：

1. 创建 ResourceManager，设置 game root 和 NLS。
2. 先加入 `data`。
3. 打开 `archive.dat`。在本地样本中它来自 `data.pac`。
4. 解析 `ARCHIVE.DAT`，把每个 path 追加进查找列表。

`ARCHIVE.DAT` 的 parser 会去掉 `\r`、`\n`、空格、tab 和 NUL，再用 `|` 分割 path。path 用 NLS 解码，`/` 统一替换成 `\`。

## 资源查找

打开资源名 `name` 时：

1. 用 NLS encode 资源名。
2. 把 `/` 改成 `\`，ASCII 小写转大写，多字节字符按原字节跳过。
3. 填入 32-byte PAC key；超过 32 byte 是错误。
4. 按 `paths` 顺序先查 loose file：`root/base/name`。
5. 再查 `root/base.pac` 中的同名 key。
6. 如果所有 base 都没有，最后查 `root/name` 和 `root.pac`。

这个顺序允许用户本地 patch loose file 覆盖 PAC 内容，也允许 `update*` path 覆盖基础资源。AstraEMU 的 Probe report 需要把命中来源标成 `loose` 或 `pac`，但不能把完整资源内容写进报告。

## PAC 结构

`sena-rs` 当前按下列布局读取 PAC：

| offset | size | meaning |
| ---: | ---: | --- |
| `0x00` | 4 | 本地样本中为 ASCII `PAC `；reader 当前不依赖 magic |
| `0x08` | 4 | 本地样本中等于 entry count；reader 用 bucket table 计算真实记录范围 |
| `0x0C` | `255 * 8` | bucket table，每项为 `first_record_index: u32` + `record_count: u32` |
| `0x804` | `record_count * 40` | record table |
| data area | variable | entry payload |

record layout：

| offset | size | meaning |
| ---: | ---: | --- |
| `0x00` | 32 | PAC key，NUL padded |
| `0x20` | 4 | payload size, little-endian u32 |
| `0x24` | 4 | payload offset, little-endian u32 |

bucket index 等于 key 的第一个 byte。`0xFF` 不能放进 255-bucket table，`sena-rs` 把它作为 unsupported bucket 处理。AstraEMU 也应在 Probe 阶段报告这种情况，而不是继续猜测。

## `$` 资源

`POINT.DAT`、`FILE.DAT`、`TEXT.DAT`、`MEM.DAT`、`GRAPHIC.DAT` 在本地样本中带 `$` marker。`sena-rs` 的解码函数只处理本地已授权资源读取后的内存 buffer：如果首 byte 是 `$` 且长度至少 `0x10`，保留前 `0x10` header，对后续 payload 做 PAL transform；如果不是 `$`，原样返回。

AstraEMU 文档和 report 不提供面向终端用户的批量解包步骤。core 内部可以实现同等 reader，用于合法本地运行、hash、诊断和测试 fixture。

## 错误模型

Archive reader 至少要区分这些错误，方便 Release Gate 定位：

| error | 触发条件 |
| --- | --- |
| `PacTooSmall` | 文件小于 `0x804` |
| `PacBucketOutOfRange` | bucket 指向 record table 外 |
| `PacDataOutOfRange` | payload offset + size 越过文件尾 |
| `NameTooLong` | encoded name 超过 32 byte |
| `InvalidArchiveDat` | `ARCHIVE.DAT` 为空或没有 path |
| `AssetNotFound` | 所有 path 和 root fallback 都未命中 |

## AstraEMU 映射

SoftPAL core 不把 PAC reader 暴露给 Manager。Core 只输出：

```text
ResourceCatalog {
  family: "softpal",
  nls: "sjis",
  search_paths: ["data", "movie", "bgm", "..."],
  archives: [{ name: "data.pac", entries: 323, sha256: "..." }],
  core_assets: [{ name: "SCRIPT.SRC", size: 3766020, sha256: "..." }]
}
```

hash 可以进 report；payload、完整文本和解包目录不能进 report。
