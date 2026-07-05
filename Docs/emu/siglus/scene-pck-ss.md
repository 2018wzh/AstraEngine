# Scene.pck And .ss Mapping

`Scene.pck` 负责把多个 `.ss` scene chunk 聚合进一个 package。AstraEMU 的读取目标不是把商业脚本导出成文本，而是在 core 内重建原引擎可执行的 scene stream，并输出本地结构化 trace。

## Package 层

Package header 的 23 个基础字段都是 little-endian `i32`。offset 均相对 `Scene.pck` 文件起点；`CIndex`/`HEADERPAIR` 记录 `{ offset, size_or_count }`。

```text
PackScnHeader
  header_size
  inc_prop_list_ofs, inc_prop_cnt
  inc_prop_name_index_list_ofs, inc_prop_name_index_cnt
  inc_prop_name_list_ofs, inc_prop_name_cnt
  inc_cmd_list_ofs, inc_cmd_cnt
  inc_cmd_name_index_list_ofs, inc_cmd_name_index_cnt
  inc_cmd_name_list_ofs, inc_cmd_name_cnt
  scn_name_index_list_ofs, scn_name_index_cnt
  scn_name_list_ofs, scn_name_cnt
  scn_data_index_list_ofs, scn_data_index_cnt
  scn_data_list_ofs, scn_data_cnt
  scn_data_exe_angou_mod
  original_source_header_size
```

Package 级 include 表会补进 scene runtime：

| 表 | entry | 作用 |
| --- | --- | --- |
| `inc_prop_list` | `{ form: i32, size: i32 }` | 跨 scene user property |
| `inc_cmd_list` | `{ scn_no: i32, offset: i32 }` | 跨 scene user command entry |
| `inc_prop_name_map` | UTF-16LE indexed names | trace 和 decompiler symbol |
| `inc_cmd_name_map` | UTF-16LE indexed names | command name lookup |

Rewrite_PLUS 有 `inc_prop_cnt=49`、`inc_cmd_cnt=148`。anemoi 体験版没有 package-level prop，但有 `inc_cmd_cnt=614`。

## Scene data entry

`scn_data_index_list[i]` 指向第 `i` 个 scene chunk：

```text
absolute_start = scn_data_list_ofs + entry.offset
absolute_end   = absolute_start + entry.size
```

Rewrite_PLUS 前几个 packed entry：

| index | offset | packed_size | scene name |
| ---: | ---: | ---: | --- |
| 0 | 0 | 17,712 | `__va_effect_ss_cmd_particle` |
| 1 | 17,712 | 6,421 | `ed_akane` |
| 2 | 24,133 | 5,316 | `ed_chihaya` |
| 3 | 29,449 | 1,726 | `ed_common` |
| 7 | 55,450 | 1,585 | `seen01000` |

这些只是索引元数据，不包含脚本文本。

## .ss chunk 层

解包后的 scene chunk 以 `S_tnm_scn_header` 开始。`siglus_rs` 和旧 string tool 一致使用 33 个 little-endian `i32` 字段，基础大小 132 bytes。

```text
S_tnm_scn_header
  header_size
  scn_ofs, scn_size
  str_index_list_ofs, str_index_cnt
  str_list_ofs, str_cnt
  label_list_ofs, label_cnt
  z_label_list_ofs, z_label_cnt
  cmd_label_list_ofs, cmd_label_cnt
  scn_prop_list_ofs, scn_prop_cnt
  scn_prop_name_index_list_ofs, scn_prop_name_index_cnt
  scn_prop_name_list_ofs, scn_prop_name_cnt
  scn_cmd_list_ofs, scn_cmd_cnt
  scn_cmd_name_index_list_ofs, scn_cmd_name_index_cnt
  scn_cmd_name_list_ofs, scn_cmd_name_cnt
  call_prop_name_index_list_ofs, call_prop_name_index_cnt
  call_prop_name_list_ofs, call_prop_name_cnt
  namae_list_ofs, namae_cnt
  read_flag_list_ofs, read_flag_cnt
```

Chunk 内 offset 均相对 chunk 起点。

## 字符串表

`str_index_list` 是 `{ offset: i32, size: i32 }` 数组，offset/size 以 UTF-16 code unit 计，不是 byte。`str_list` 存 UTF-16LE word。scene 字符串按逻辑序号做 per-word XOR：

```text
decoded_word = stored_word XOR ((28807 * string_index) as u16)
```

旧 `stringdump`/`stringpacker` 和 `siglus_rs` 都使用这个口径。AstraEMU report 不输出 decoded text 全文，只输出文本事件长度、hash、read flag 和当前位置。

## Label 与 command entry

| 表 | entry | VM 用途 |
| --- | --- | --- |
| `label_list` | `i32 offset` | `CD_GOTO`/`CD_GOSUB` label target |
| `z_label_list` | `i32 offset` | `jump(scene, z)`、`farcall(scene, z)` 的 z label |
| `cmd_label_list` | `{ cmd_id: i32, offset: i32 }` | user command label |
| `scn_cmd_list` | `i32 offset` | scene-local command entry |

Bytecode 的合法入口至少包括 offset 0、scene command offsets、label table、z-label table 和 command label table。反汇编可以多入口遍历；运行时只按当前 VM 控制流执行。

## AstraEMU 内存形态

Core 启动后保留：

```text
ScenePackage
  header
  scene_names: Vec<String>
  package_inc_props
  package_inc_cmds
  rebuilt_scene_data: Vec<u8>

SceneStream
  chunk: &[u8]
  header: ScnHeader
  code: &[u8]
  string_index_list
  label_list
  z_label_list
  pc
```

`ScenePackage` 是 core 私有对象。Manager 只能通过 `StateMachineTrace` 看到 scene number、scene name、line number、pc、command name/hash 和资源引用。
