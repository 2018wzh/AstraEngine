# Minori PAZ Details

## Header/TOC

当前实现阶段只固定 reader contract，不固定未验证字段名：

```text
PazHeader
  magic_or_seed
  archive_flags
  toc_offset
  toc_size
  entry_count
PazEntry[]
  name_or_name_hash
  offset
  packed_size
  unpacked_size
  method_flags
```

`Tools/AstraEMU/minori_paz.py` 先输出文件 hash、head bytes 和 key 是否提供。完成 TOC 验证后再把字段名提升到 `archive-format.md`。

## Payload Method

历史工具显示 PAZ payload 可能叠加：

- TOC obfuscation。
- Blowfish-like block transform。
- zlib compression。
- per-file lightweight xor/rotate。

AstraEMU core 中这些步骤必须拆成可 trace 的 stage：

```text
ReadRaw -> DecodeToc -> ResolveEntry -> DecodePayload -> VerifyHash -> ExposeReadOnlyBytes
```

每个 stage 输出 byte count、method flag、hash 和失败 diagnostic。调试报告不输出 payload。

## Example

本地 case：

```text
root = <minori-case-root>
archive = scr.paz
role = script
expected output = entry list + script candidates
```

没有 key 时工具输出：

```text
key_supplied = false
note = TOC decode is intentionally key-driven
```

这就是可接受状态，不是失败。
