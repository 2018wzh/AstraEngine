# FVP Source Inventory

本页索引完整 RFVP submodule 的原生模块与样本 metadata。当前固定提交和差异分类见 [MODIFICATIONS](https://github.com/2018wzh/rfvp/blob/95d3ee23548cb32e035aaaaeab2d309fc5d3020e/crates/astra-emu-rfvp/MODIFICATIONS.md)，实现状态见[性能审计](../../status/performance-upstream-audit-2026-10-12.md)。

## FVP 参考入口

以下路径相对 `Emulator/ThirdParty/rfvp`。AstraEMU 直接复用原实现，不按此表再实现一套 VM、decoder 或 renderer。

| 原生路径 | 职责 | 适配边界 |
| --- | --- | --- |
| `crates/rfvp/src/script/parser.rs` | HCB header、NLS、syscall、screen mode | 复用原解析与诊断 |
| `crates/rfvp/src/script/opcode.rs` | opcode 定义 | 原 VM 执行，不新增 Host trace 协议 |
| `crates/rfvp/src/script/context.rs` | 栈、Variant、ConstString、dispatch | 保持原操作语义 |
| `crates/rfvp/src/vm_runner.rs` | 协程、yield、保存恢复安全点 | 功能修复与嵌入通知分别审查 |
| `crates/rfvp/src/subsystem/resources/vfs.rs` | BIN、loose override、资源查找 | Family 提供文件访问，原核心解析 |
| `crates/rfvp/src/subsystem/world.rs` | GameData 与 syscall registry | 会话自持，不进入 RuntimeWorld |
| `crates/rfvp/src/subsystem/components/syscalls/` | graph、text、sound、movie、thread 命令 | 复用原实现 |
| `crates/rfvp/src/rendering/gpu_prim.rs` | 原生 primitive GPU 绘制 | 原 renderer，不转为 Host 场景命令 |
| `crates/rfvp/src/no_std_core.rs` / `host_api/` | 核心运行与嵌入端口 | 输入/时间、资源、字体、音频和生命周期 |

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
