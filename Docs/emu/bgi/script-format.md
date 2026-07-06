# BGI Script Format

BGI script 不能只靠文件扩展名判断。archive entry 常见外层是 `DSC FORMAT 1.00`，core 必须先解包，再对 decoded bytes 做格式检测。

## 检测顺序

`BgiScriptProbe` 应按以下顺序返回：

1. decoded bytes 以 `BurikoCompiledScriptVer1.00\0` 开头：`BgiScriptKind::BcsV1`。
2. entry name 以 `._bp` 结尾，且 bytes 满足 BP header 或 headerless BP 规则：`BgiScriptKind::BpProgram`。
3. bytes 前 8 bytes 可解释为 `header_size` 和 `instruction_size`，且 `header_size >= 8`、`header_size <= len`、`instruction_size <= len`、`header_size + instruction_size == len`：优先视为 BP。
4. extensionless scenario bytes 非空但没有 BCS magic：`BgiScriptKind::HeaderlessScenario`，只允许在 case profile 明确支持时进入 VM。
5. 其他 payload 返回 `UnknownScript`，写入 case report。

`<bgi-modern-case>/data01100.arc:00_op_01` 是 DSC-wrapped BCS：raw size 34,372 bytes，decoded size 114,699 bytes，decoded header 的 `body_start` 为 `0x40`。`<bgi-packfile-case>/system.arc:ipl._bp` 是 DSC-wrapped BP：raw size 1,983 bytes，decoded BP length 2,608 bytes。

## 字符串编码

- 原版 BGI payload 以 CP932/Shift_JIS 兼容解码为主。
- 翻译工具中出现的 GBK/CP936 逻辑只说明某些本地化补丁如何写回文本，不是 runtime 默认规则。
- core 的 string decoder 应返回 `BgiDecodedString { bytes, text, encoding, diagnostics }`。解码失败时保留 bytes，不丢弃 source map。
- 文档和 trace 可以记录短字符串类别、长度、offset 和 hash；不能复制完整商业 script 文本。

## Source map

每条 VM command 至少保留：

- `archive_path`：相对 game root 的 archive 路径。
- `entry_name`：archive 内 name。
- `raw_offset`：entry 在 archive data block 中的 absolute offset。
- `decoded_offset`：解码后 payload 内 offset。
- `vm_address`：BCS 使用 body-relative address；BP 使用 file/code offset。
- `line_hint`：BCS `0x7F` debug command 的 line number；没有则为空。

source map 是 replay、trace、error report 和 text capture 的基础定位单位。它不能包含私有开发机绝对路径。

## Runtime 分类

| Kind | 主要用途 | 入口 |
| --- | --- | --- |
| `BcsV1` | scenario、message、choice、graph/sound command | BCS sub table 的 `main` 或调用目标。 |
| `BpProgram` | system program、resource helper、message system、launcher | `system.arc`、`sysprg.arc` 中的 `._bp`。 |
| `HeaderlessScenario` | 较早或本地化发行中的 compact scenario | case profile 指定入口。 |
| `UnknownScript` | 新变种或非 script payload | case report，不执行。 |
