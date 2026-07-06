# BGI Game Observations

本页记录三个本地游戏目录的 metadata 观测。所有数字来自 header、entry table、payload magic 和局部 parser 统计；未复制完整商业 script、图像、音频或影片内容。后续验收报告应使用 case id 和相对路径，不输出这些开发机绝对根目录。

## `<bgi-packfile-case>`

总体：

- top-level 文件 18 个，总文件 29 个。
- `.arc` 12 个，全部为 `PackFile`。
- archive entry 总数 1,925。
- payload magic 统计：`DSC` 117、`CBG` 1,621、`BurikoWaveBox` 140、`BP` 47。

代表 archive：

| Archive | Format | Entries | Data base | Payload |
| --- | --- | ---: | ---: | --- |
| `data01100.arc` | `PackFile` | 5 | `0xB0` | scenario-like `DSC` |
| `data01101.arc` | `PackFile` | 7 | `0xF0` | scenario-like `DSC` |
| `data02200.arc` | `PackFile` | 998 | `0x7CD0` | `CBG` |
| `data02201.arc` | `PackFile` | 122 | `0xF50` | `CBG` |
| `data02202.arc` | `PackFile` | 437 | `0x36B0` | `CBG` |
| `data03300.arc` | `PackFile` | 99 | `0xC70` | `BurikoWaveBox` |
| `data05500.arc` | `PackFile` | 25 | `0x330` | BGM `BurikoWaveBox` |
| `sysgrp.arc` | `PackFile` | 105 | `0xD30` | system graph `DSC` |
| `sysprg.arc` | `PackFile` | 45 | `0x5B0` | `._bp` system program |
| `system.arc` | `PackFile` | 2 | `0x50` | `ipl._bp`、`launcher._bp` |

具体样例：

- `data01100.arc:main`：absolute offset `0xB0`，relative offset `0x0`，raw size 726 bytes。
- `data01100.arc:skp001`：absolute offset `0x386`，relative offset `0x2D6`，raw size 31,070 bytes。
- `data03300.arc:se001`：absolute offset `0xC70`，raw size 7,205 bytes，header 起始为 `40 00 00 00 62 77 20 20`。
- `system.arc:ipl._bp`：raw DSC size 1,983 bytes，decoded BP length 2,608 bytes，`header_size=16`，`instruction_size=2592`，code start `0x10`。

注意：该本地化发行的部分 scenario-like DSC payload 解码后不一定是 BCS magic。case profile 需要允许 compact/headerless scenario 分支，并在未确认前只输出 diagnostic。

## `<bgi-modern-case>`

总体：

- top-level 文件 58 个，总文件 68 个。
- `.arc` 54 个，其中 `PackFile` 6 个、`BURIKO ARC20` 48 个。
- archive entry 总数 50,807。
- payload magic 统计：`DSC` 348、`CBG` 22,580、`MPEG` 2、`BurikoWaveBox` 27,813、`BP` 58、`Unknown` 6。

代表 archive：

| Archive | Format | Entries | Data base | Payload |
| --- | --- | ---: | ---: | --- |
| `data01099.arc` | `BURIKO ARC20` | 23 | `0xB90` | scenario `DSC` |
| `data01100.arc` | `BURIKO ARC20` | 5 | `0x290` | scenario `DSC` |
| `data02100.arc` | `BURIKO ARC20` | 260 | `0x8210` | `CBG` |
| `data02306.arc` | `BURIKO ARC20` | 4,634 | `0x90D10` | `CBG` |
| `data03800.arc` | `BURIKO ARC20` | 321 | `0xA090` | `BurikoWaveBox` |
| `data04100.arc` | `BURIKO ARC20` | 4,273 | `0x85890` | voice `BurikoWaveBox` |
| `data05000.arc` | `BURIKO ARC20` | 50 | `0x1910` | BGM `BurikoWaveBox` |
| `sysgrp.arc` | `BURIKO ARC20` | 156 | `0x4E10` | system graph `DSC` |
| `sysprg.arc` | `BURIKO ARC20` | 57 | `0x1C90` | `._bp` system program |
| `system.arc` | `BURIKO ARC20` | 3 | `0x190` | BP 1、DSC 2 |

具体样例：

- `data01100.arc:00_op_01`：absolute offset `0x290`，raw DSC size 34,372 bytes，decoded BCS length 114,699 bytes。
- `data01100.arc:00_op_02`：absolute offset `0x88D4`，relative offset `0x8644`，raw size 28,244 bytes。
- `data05000.arc:bgm001`：raw size 7,228,532 bytes，`BurikoWaveBox`。
- `sysprg.arc:scrmsg._bp`：raw DSC size 8,365 bytes，decoded BP length 19,904 bytes，code start `0x10`。

`00_op_01` 的 BCS parser 观测：

- `header_size=36`，`body_start=0x40`。
- namespace count 1，namespace `Yuzu_2G`。
- sub count 1，`main@0`。
- `code_end=0x11750`。
- command count 约 8,936。

## `<bgi-15th-case>`

总体：

- `.arc` 53 个，全部为 `BURIKO ARC20`。
- archive entry 总数 39,142。
- payload magic 统计：`DSC` 473、`MPEG` 5、`CBG` 5,152、`BurikoWaveBox` 33,454、`BP` 56、`Unknown` 2。

代表 archive：

| Archive | Format | Entries | Data base | Payload |
| --- | --- | ---: | ---: | --- |
| `data01000.arc` | `BURIKO ARC20` | 1 | `0x90` | `yuzu_2g` DSC |
| `data01101.arc` | `BURIKO ARC20` | 10 | `0x510` | scenario `DSC` |
| `data01701.arc` | `BURIKO ARC20` | 3 | `0x190` | scenario `DSC` with non-zero tail |
| `data02100.arc` | `BURIKO ARC20` | 1 | `0x90` | MPEG `carsed.mpg` |
| `data02101.arc` | `BURIKO ARC20` | 1 | `0x90` | MPEG `op.mpg` |
| `data02920.arc` | `BURIKO ARC20` | 3,349 | variable | `CBG` |
| `data02931.arc` | `BURIKO ARC20` | 1 | variable | `BF_Movie____` sample |
| `data04010.arc` | `BURIKO ARC20` | 5,811 | `0xB5990` | voice `BurikoWaveBox` |
| `data05000.arc` | `BURIKO ARC20` | 50 | variable | BGM `BurikoWaveBox` |
| `installer.arc` | `BURIKO ARC20` | 6 | `0x310` | BP 1、DSC 5 |
| `sysgrp.arc` | `BURIKO ARC20` | 338 | `0xA910` | system graph `DSC` |
| `sysprg.arc` | `BURIKO ARC20` | 54 | `0x1B10` | `._bp` system program |
| `system.arc` | `BURIKO ARC20` | 1 | `0x90` | `ipl._bp` |

具体样例：

- `data01101.arc:1-1_0710_dream`：raw DSC size 5,169 bytes，decoded BCS length 16,694 bytes。
- `data01101.arc:1-1_0712_dream`：absolute offset `0x1941`，relative offset `0x1431`，raw size 12,172 bytes。
- `data02101.arc:op.mpg`：raw size 99,102,384 bytes，MPEG start code。
- `data02931.arc:guruguru.bm`：payload magic `BF_Movie____`。
- `data04010.arc:yuki_000001`：absolute offset `0xB5990`，raw size 18,671 bytes，`BurikoWaveBox`。
- `system.arc:ipl._bp`：raw DSC size 2,283 bytes，decoded BP length 2,400 bytes，`header_size=16`，`instruction_size=2384`。

`1-1_0710_dream` 的 BCS parser 观测：

- `header_size=36`，`body_start=0x40`。
- namespace count 0。
- sub count 1，`main@0`。
- `code_end=0x302C`。
- command count 约 1,547。

## 验收含义

- archive reader 必须同时覆盖 `PackFile` 与 `BURIKO ARC20`。
- payload decoder 必须先覆盖 DSC，再覆盖 BCS/BP、CBG、audio box 和 MPEG probe。
- BGI core 初期 smoke test 可以使用这些 metadata 断言，不需要读取或保存完整商业 payload。
- `Unknown` magic 不是自动失败；case report 应列出 archive、entry、offset、size、magic 和是否影响启动路径。
