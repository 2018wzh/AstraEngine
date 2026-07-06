# SoftPAL Implementation Checklist

本页是 SoftPAL family 的实现清单，不表示当前 AstraEngine 仓库已经实现这些 runtime crate。每项完成后需要有对应 diagnostics、fixture 或 local case report。

## Probe and resource catalog

- [ ] 识别 `SoftPAL` family：`data.pac`、`ARCHIVE.DAT`、`SCRIPT.SRC` magic `Sv20`。
- [ ] 支持 NLS：`sjis`、`gbk`、`utf-8`，默认 `sjis`。
- [ ] 解析 `ARCHIVE.DAT` path list，报告存在和缺失 path。
- [ ] 读取 PAC bucket/record metadata，校验越界。
- [ ] 打开核心资源：`Script.src`、`Point.dat`、`File.dat`、`Text.dat`、`Mem.dat`。
- [ ] 对 `$` resource 做内部解码，report 只写 hash 和 size。

## Script VM

- [ ] 解析 `Sv20` header、check value、entry PC。
- [ ] 解析 `POINT.DAT` reverse point id。
- [ ] 实现 primary opcode：move、arithmetic、compare、branch、call、return、push、pack/drop args。
- [ ] 实现 operand banks：immediate、user/system/temp memory、var、stack、argument、Mem.dat direct。
- [ ] 对 `MemDatIndirect` 给出明确 diagnostic，未实现时不算 full route。
- [ ] VM step 支持 instruction budget。
- [ ] wait 统一输出 serializable `AwaitToken`。

## Extcalls

- [ ] 引入 signature table：category/index/name/pop_count/status。
- [ ] text category：text init/show/hide/base/reveal/voice link。
- [ ] sprite category：set、position、alpha、scale、transition、face。
- [ ] audio categories：BGM、SE、voice/BGV load/play/stop/volume/wait。
- [ ] select/button/system menu categories。
- [ ] save/load/history categories。
- [ ] movie/MSP category。
- [ ] file/profile/string helpers 做 capability check。
- [ ] unknown extcall 清理 stack 并输出 concern。

## Presentation and media

- [ ] 解码 PGD `GE ` base image。
- [ ] 解码 `PGD3` delta image 和 base resolver。
- [ ] 解码 TGA；mask BMP 走 media provider 或 simple decoder。
- [ ] sprite state 转 `PresentationCommand`。
- [ ] text capture 默认只输出 id、hash、长度和 speaker metadata。
- [ ] OGG audio 转 `AudioCommand`。
- [ ] MPG/movie path 通过 media provider，不加载 legacy DLL。

## Snapshot and save

- [ ] Snapshot 保存 PC、call stack、VM stack、argument stack。
- [ ] Snapshot 保存 user/system/temp memory。
- [ ] Snapshot 保存 `mem_dat_words` shadow。
- [ ] Snapshot 保存 text/history/select/button 关键状态。
- [ ] Snapshot 不保存 native renderer/audio/window handle。
- [ ] Save/load route 输出 hash 和 version。

## Release gate

- [ ] `probe` report 不泄露绝对路径和 payload。
- [ ] `boot_to_wait` 能到首个稳定 wait。
- [ ] `adv_text` 捕获 text id、voice id、window visible。
- [ ] `audio` 覆盖 BGM、SE、voice。
- [ ] `presentation` 覆盖 sprite count、logical size、scene hash。
- [ ] `save_load` 验证 snapshot restore 后 VM state 一致。
- [ ] `extcall_coverage` 给出 known、partial、blocked、unknown 聚合。
- [ ] 所有 concern 都有 PC/category/index/resource context。

## Done criteria

SoftPAL family 只能在这些条件同时满足后进入 `DONE`：

- local case report 通过 release gate；
- no payload/no screenshot/no absolute private path 检查通过；
- unknown 或 partial extcall 不影响被声明通过的 route；
- snapshot restore 和 replay 不重新请求非 deterministic provider；
- Family plugin provider boundary 中没有 legacy pointer、DLL handle、GPU handle 或 audio handle。
