# Gameexe Config

`Gameexe.dat` 是 Siglus 的运行时配置入口。它影响窗口、资源表、系统菜单、message window、save/load/config scene、音频、CG table、font 和各种 UI 参数。AstraEMU core 必须把它当作只读配置表，而不是普通文本旁路文件。

## 文件变体

样本中出现：

| 文件 | 样本 |
| --- | --- |
| `Gameexe.dat` | anemoi 体験版、Rewrite_PLUS |
| `Gameexe.chs` | Rewrite_PLUS 汉化版 |

Resolver 应枚举 `Gameexe.*`，按启动 exe/locale/用户选择确定主配置。没有选择信息时，优先 `Gameexe.dat`，同时把其它 `Gameexe.*` 作为 diagnostics 列出。

## Header

原始编译格式以 8 bytes header 开始：

```text
i32 version
i32 exe_angou_mode
u8  body[]
```

两个样本的 first 8 bytes 都是：

```text
00 00 00 00 01 00 00 00
```

解释为：

```text
version = 0
exe_angou_mode = 1
```

`exe_angou_mode != 0` 表示 body 需要 title-specific 授权材料参与 decode。AstraEMU 不提供材料提取流程；只消费用户合法提供的配置，并在缺失时报告 `config_decode_status=blocked`。

## Decode pipeline

参考实现把 pipeline 分为几类：

1. 带 header 的原始格式。
2. 明文 UTF-16LE、Shift-JIS 或 UTF-8 文本。
3. headerless LZSS/intermediate 文件。
4. 可选授权 XOR chain 后再尝试 LZSS 和文本解码。

文档不记录固定 key 内容。实现上应把 decode material 封装成 `SiglusDecodeMaterial`：

```text
SiglusDecodeMaterial
  exe_key16: Option<[u8; 16]>
  base_code: Option<opaque bytes>
  game_code: Option<opaque bytes>
  chain_order: Vec<DecodeStepKind>
```

该对象只存在 core 内存，不出 IPC，不进 report。

## 文本格式

展开后是 INI-like 文本。解析规则：

| 规则 | 说明 |
| --- | --- |
| 空行跳过 | 不生成 entry |
| `;` 行跳过 | 注释 |
| 行首 BOM 跳过 | 支持 UTF-16/UTF-8 BOM |
| 行首 `#` 可省略 | 查询时 `MSGBK.X` 与 `#MSGBK.X` 等价 |
| key normalize | trim、去空白、转 ASCII uppercase |
| `=` 分隔 | 左侧 key，右侧 value |
| `;` inline comment | 只在非字符串内生效 |
| CSV-like value | 逗号分割，但引号内逗号保留 |
| duplicated key | 查询单值时后定义覆盖前定义 |

非商业示例来自参考实现测试：

```text
#MSGBK.WINDOW_SIZE = 1280, 720
#MSGBK.BACK_FILE = "mn_mw_log00a00"
#WAKU.000.EXTEND_TYPE = 2
#COLOR_TABLE.000 = 255, 255, 255
```

查询口径：

| API | 行为 |
| --- | --- |
| `get_value("MSGBK.WINDOW_SIZE")` | 返回完整 `1280, 720` |
| `get_unquoted("MSGBK.BACK_FILE")` | 返回 `mn_mw_log00a00` |
| `get_indexed_field("WAKU", 0, "EXTEND_TYPE")` | 匹配 `WAKU.000.EXTEND_TYPE` |
| `get_indexed_value("COLOR_TABLE", 0)` | 返回完整三元组 |

Indexed lookup 必须按解析出的数字 index 匹配，不要把 index 重新格式化成非补零字符串，否则 `WAKU.000` 这类键会漏掉。

## Runtime 依赖

首阶段需要读取的 key family：

| family | 作用 |
| --- | --- |
| `SAVE_SCENE`/`LOAD_SCENE`/`CONFIG_SCENE` | 系统菜单 scene 和 z label |
| `MSGBK.*` | backlog/message window layout |
| `WAKU.*` | message frame 和窗口 skin |
| `COLOR_TABLE.*` | text/color table |
| `BGM.*` | BGM table |
| `CGTABLE`/`CGTABLE2` references | gallery/CG table |
| `THUMB*`/save thumbnail keys | save/load 缩略图 |
| font/config keys | 字体列表、文字速度、音量、鼠标行为 |

## Diagnostics

Decode 成功：

```json
{
  "gameexe": "Gameexe.dat",
  "version": 0,
  "exe_angou_mode": 1,
  "entry_count": 1234,
  "encoding": "Utf16Le",
  "used_lzss": true
}
```

Decode 失败：

```json
{
  "gameexe": "Gameexe.dat",
  "version": 0,
  "exe_angou_mode": 1,
  "config_decode_status": "blocked",
  "reason": "missing authorized decode material"
}
```

不要输出展开后的完整 config。
