# FVP Source Inventory

## FVP 参考入口

FVP 研究实现是本次设计输入的主要参考。AstraEMU 只吸收格式、状态机和验收经验，不复制该实现的产品结构。

| 路径 | 可用事实 | AstraEMU 用法 |
| --- | --- | --- |
| `crates/rfvp/src/script/parser.rs` | `.hcb` header、`Nls`、syscall table、screen mode 映射 | 编写 `FvpScriptHeader`、probe 和 diagnostics |
| `crates/rfvp/src/script/opcode.rs` | 0x00..0x27 opcode 名称 | 定义 bytecode decoder 和 trace vocabulary |
| `crates/rfvp/src/script/context.rs` | stack VM、`Variant`、`ConstString` offset、`VmSyscall` dispatch | family plugin VM 执行模型 |
| `crates/rfvp/src/vm_runner.rs` | frame tick、thread state、yield、save/load safe point | AstraEMU RuntimeWorld tick 边界和 ordered event 输出 |
| `crates/rfvp/src/subsystem/resources/vfs.rs` | `.bin` pack parser、loose file override、VFS path 规范化 | archive reader 和 media resolver |
| `crates/rfvp/src/subsystem/world.rs` | `GameData` 资源集合和 syscall registry | family-private 状态聚合，不进入 EngineCore |
| `crates/rfvp/src/subsystem/components/syscalls/*.rs` | graph、text、sound、movie、thread 等 syscall 行为 | legacy API mapper 和兼容矩阵 |
| `crates/rfvp/src/subsystem/resources/*` | prim、motion、text、graph buffer、save manager | snapshot section 和 PresentationCommand 来源 |
| `crates/rfvp/src/portable/*` | portable VM、host bridge、no_std host API | 可借鉴 core/host 分离，不继承平台限制 |

## 工具入口

| 工具 | 路径 | 适用边界 |
| --- | --- | --- |
| disassembler | `crates/disassembler` | `.hcb` -> project YAML/disassembly YAML，用于 metadata 和局部 opcode 诊断 |
| assembler | `crates/assembler` | project YAML -> `.hcb`，仅用于 round-trip test，不用于改写商业样本 |
| hcb2lua | `crates/hcb2lua_decompiler` | first-pass Lua 输出，适合本地结构化控制流审计 |
| lua2hcb | `crates/lua2hcb_compiler` | 受限 Lua-like 合约回编，适合 synthetic fixture |
| nvsg_pack | `crates/nvsg_pack` | FAVORITE `HZC1 + NVSG` texture pack/unpack，适合公开 fixture 和自制资源 |

## 本地样本清单

合法本地样本在文档中统一写 `<game-root>`，不记录本机绝对路径。

| 文件 | 字节数 | 观察 |
| --- | ---: | --- |
| `Sakura.hcb` | 5,002,852 | 主脚本，Shift_JIS title，148 个 syscall，custom syscall count 为 0 |
| `bgm.bin` | 984,835,075 | 70 个 entry，entry payload 以 `OggS` 开头 |
| `voice.bin` | 1,069,344,978 | 14,498 个 entry，entry payload 以 `OggS` 开头 |
| `se.bin` | 105,872,097 | 304 个 entry，entry payload 以 `RIFF` 开头 |
| `se_env.bin` | 98,729,276 | 79 个 entry，entry payload 以 `RIFF` 开头 |
| `se_sys.bin` | 3,309,764 | 13 个 entry，entry payload 以 `RIFF` 开头 |
| `graph.bin` | 354,017,024 | 1,146 个 entry，entry payload 以 `hzc1` 开头 |
| `graph_bg.bin` | 703,338,219 | 375 个 entry，entry payload 以 `hzc1` 开头 |
| `graph_bs.bin` | 798,099,049 | 594 个 entry，entry payload 以 `hzc1` 开头 |
| `graph_sd.bin` | 427,708,462 | 57 个 entry，entry payload 以 `hzc1` 开头 |
| `graph_vis.bin` | 2,008,972,031 | 579 个 entry，entry payload 以 `hzc1` 开头 |
| `graph_vish.bin` | 1,116,635,151 | 380 个 entry，entry payload 以 `hzc1` 开头 |
| `patch.bin` | 31,634,830 | 71 个 entry，entry payload 以 `hzc1` 开头，来自本地汉化样本 |
| `movie/01.wmv` | 111,307,131 | ASF/WMV loose movie |
| `movie/02.wmv` | 130,225,633 | ASF/WMV loose movie |
| `cursor1.ani` | 2,314 | loose ANI cursor |
| `cursor2.ani` | 78,678 | loose ANI cursor |

## 不进入 AstraEMU core 的内容

`Sakura.exe`、`SakuraChs.exe`、`launch.exe`、`filter.dll`、`libass-9.dll`、安装器、卸载器和补丁安装包都不属于 family plugin 输入。FVP plugin 只需要合法安装后的 data root、`.hcb`、`.bin`、loose movie、loose cursor 和可选字幕 metadata。对可执行文件的行为观察只能进入本地结构化 diagnostics。
