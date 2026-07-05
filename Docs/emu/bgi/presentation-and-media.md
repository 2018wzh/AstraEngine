# Presentation and Media

BGI presentation adapter 把 graph/sound/movie command 转成 AstraEMU 可记录的 presentation patch 和 media block。旧引擎内部对象、surface 指针或平台 handle 不能跨进程传递。

## `CompressedBG___`

CBG image header：

| Offset | Size | Type | 说明 |
| --- | ---: | --- | --- |
| `0x00` | `0x10` | bytes | magic `CompressedBG___`。 |
| `0x10` | `0x02` | `u16le` | width。 |
| `0x12` | `0x02` | `u16le` | height。 |
| `0x14` | `0x04` | `u32le` | bpp。 |
| `0x18` | `0x08` | bytes | reserved/unknown。 |
| `0x20` | `0x04` | `u32le` | intermediate length。 |
| `0x24` | `0x04` | `u32le` | key。 |
| `0x28` | `0x04` | `u32le` | encoded length。 |
| `0x2C` | `0x01` | byte | checksum/sum。 |
| `0x2D` | `0x01` | byte | xor。 |
| `0x2E` | `0x02` | `u16le` | version。 |

CBG v1/v2 的输出统一为 RGBA。已知组合包括 `(version=1, bpp=8/16/24/32)` 与 `(version=2, bpp=8/24/32)`。v2 使用 DCT table、Huffman tree、block offset 和 64 项 block fill order；implementation 可以先只暴露 decoder diagnostic，再逐步覆盖全组合。

## Raw BGI image

raw image 不是 CBG magic，而是直接以小 header 开始：

| Offset | Size | Type | 说明 |
| --- | ---: | --- | --- |
| `0x00` | `0x02` | `u16le` | width。 |
| `0x02` | `0x02` | `u16le` | height。 |
| `0x04` | `0x02` 或 `0x04` | integer | depth/bpp。 |
| `0x10` | variable | bytes | pixel bits。 |

BGITextureConverter 侧证 data start 为 `0x10`。core 应支持 24/32-bit BGR(A) 到 RGBA；其他 depth 先返回 unsupported diagnostic。

## Layer classification

presentation state 按 resource name 分类，生成稳定 layer id。参考分类：

| 类别 | Name pattern | Layer |
| --- | --- | ---: |
| scenario overlay | VM text/overlay | `-50000` |
| hit/input | hit region | `50000` |
| background | `bg*` | `-60010` |
| event | `ev_*` 或 `ev*` | `-60020` |
| logo | 包含 `logo` 或 `brandlogo` | `-60030` |
| UI | `msg*` 或 `sys*` | `-60040` |
| effect | `ef_*` 或 `ef*` | `-60200` 起 |
| fade plate | `bg_black`、`bg_white` | `-60290` |
| character | `ll_`、`l_`、`m_`、`s_`、`lm_`、`ml_`、`mm_`、`sl_`、`sm_` 等 | `-60400` 起 |
| other | 未分类对象 | `-60700` 起 |

opacity 建议：

- fade plate：`1.0`。
- `ef_soft*`：`0.28`。
- `ef_sepia*`：`0.42`。
- 其他 effect：`0.5`。
- 普通对象：`1.0`。

resource alias fallback 可覆盖常见缺图：`ef_softN` 可回退到较小编号或 `ef_soft`；角色名遇到 `_d_` 时可尝试 body/face 候选。fallback 必须写入 diagnostic，不能静默改名。

## Audio

`BurikoWaveBox` header：

| Offset | Size | Type | 说明 |
| --- | ---: | --- | --- |
| `0x00` | `0x04` | `u32le` | header length，常见 `0x40`。 |
| `0x04` | `0x04` | bytes | ASCII `bw  `。 |
| `0x08` | `0x04` | `u32le` | file size。 |
| `0x0C` | `0x04` | `u32le` | sample length。 |
| `0x10` | `0x04` | `u32le` | frequency，常见 44,100。 |
| `0x14` | `0x04` | `u32le` | channels，常见 2。 |
| `header_len` | variable | bytes | OggS 或 RIFF payload。 |

示例：`E:\Games\サクラノ詩\data05000.arc:bgm001` 是 `BurikoWaveBox`，raw size 为 7,228,532 bytes，header 起始为 `40 00 00 00 62 77 20 20`。core 输出 `BgiMediaBlock { kind: Audio, codec: Ogg | RiffWave, resource_id, timing }`，不跨进程传递 decoder handle。

## Movie

观测到两类 movie payload：

- MPEG stream：payload 以 `00 00 01 BA` 或 `00 00 01 B3` 等 start code 开头，例如 `E:\Games\素晴らしき日々15th\data02101.arc:op.mpg`。
- `BF_Movie____`：例如 `E:\Games\素晴らしき日々15th\data02931.arc:guruguru.bm`。该类先作为 proprietary movie container 记录 magic 和 size，不在没有公开规格时猜测解码。

movie playback 应生成 `AwaitToken(kind=Movie)`。回放使用记录的 start/complete event，不依赖平台 decoder 完成顺序。
