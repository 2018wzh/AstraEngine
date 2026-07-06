# Siglus Source Inventory

本页列出本轮文档使用的事实来源。它不是可执行逆向指南，只记录 AstraEMU 需要对齐的结构事实。

## Rust 参考实现

Siglus Rust 参考实现是主要参考。仓库 README 将其定位为非官方 Rust 实现和多平台 SiglusEngine port，包含资源解析、Scene VM、媒体播放和平台壳。

已使用的关键路径：

| 路径 | 用途 |
| --- | --- |
| `crates/siglus_assets/src/scene_pck.rs` | `Scene.pck` header、scene name table、scene data chunk rebuild、pack include prop/cmd |
| `crates/siglus_ss_decompiler/src/scene.rs` | `.ss` 内部 `S_tnm_scn_header`、字符串表、label、cmd label、prop/cmd 表 |
| `crates/siglus_ss_decompiler/src/disasm.rs` | `CD_*` bytecode operand 消费顺序和控制流入口 |
| `crates/siglus_ss_decompiler/src/constants.rs` | form、element、opcode 名称表 |
| `crates/siglus_scene_vm/src/vm.rs` | VM 栈、call frame、scene stack、savepoint、proc boundary |
| `crates/siglus_scene_vm/src/scene_stream.rs` | `.ss` runtime 读取、字符串解码、label jump、scene command entry |
| `crates/siglus_scene_vm/src/runtime/mod.rs` | `CommandContext`、媒体、输入、等待、save/load 请求 |
| `crates/siglus_scene_vm/src/runtime/opcode.rs` | numeric form dispatch hook |
| `crates/siglus_assets/src/gameexe.rs` | `Gameexe.dat` header、文本编码猜测、INI-like parser、indexed key lookup |
| `crates/siglus_assets/src/g00.rs` | G00 type 0/1/2/3、cut/chip、BGRA/JPEG/LZSS 口径 |
| `crates/siglus_assets/src/omv.rs` | OMV header 和内嵌 Ogg/Theora 偏移 |
| `crates/siglus_assets/src/ovk.rs` | OVK entry table 与 OWP stream wrapper |
| `crates/siglus_assets/src/nwa.rs` | NWA 44-byte header、unit table、PCM 输出 |
| `crates/siglus_assets/src/mpeg2.rs` | MPEG sequence header probe |
| `crates/siglus_assets/src/cgm.rs` | CG table `CGTABLE`/`CGTABLE2` 结构 |
| `crates/siglus_assets/src/dbs.rs` | DBS expanded table、row/column/data/string layout |

未纳入本文档的内容：key 数据、key 获取路径、完整反编译输出、商业文本和媒体 payload。

## 旧 C/C++ 取证代码

历史 SiglusEngine 取证代码提供了较早的格式线索。它能验证很多结构名和字段顺序，但包含 patcher、DLL、静态 byte table 和注入相关材料。AstraEMU 文档只取结构，不复制可执行绕过步骤。

已使用的关键路径：

| 路径 | 用途 |
| --- | --- |
| `SiglusEngine_patcher/SceUnPacker/SceUnPacker/Main.cpp` | `SCENEHEADER`、`HEADERPAIR`、scene name/data table、compressed scene chunk header |
| `SiglusEngine_patcher/SceUnPacker/stringdump/Main.cpp` | `.ss` 字符串索引和按字符串序号 XOR 的事实 |
| `SiglusEngine_patcher/SceUnPacker/stringpacker/Main.cpp` | 字符串表 offset/length 更新口径 |
| `SiglusEngine/g00_test.cpp` | G00 type、width/height、type2 cut/block 结构、LZSS 变体 |
| `SiglusEngine/说明.txt` | 旧工具作者对字符串、VM 指令和绘制路径的粗略备注 |

## 本地样本观测

样本只用于 header、计数、扩展名和少量资源名观测。

| 样本 | 观测范围 |
| --- | --- |
| `<siglus-anemoi-case>` | `Scene.pck` header、`Gameexe.dat` header、资源扩展名分布、G00/OMV/OVK/OWP/Ogg/WMV presence |
| `<siglus-rewrite-case>` | `Scene.pck` header、`Gameexe.dat`/`Gameexe.chs` header、资源扩展名分布、G00/OMV/OVK/NWA/Ogg/MPEG/WMV presence |

观测命令见 [tooling.md](tooling.md)。文档中只保留 machine-readable header 和文件名级例子。
