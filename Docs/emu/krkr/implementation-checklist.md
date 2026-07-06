# Implementation Checklist

本清单约束 KrKr family 的实现顺序。每项完成后再进入下一项，避免先写 UI 或插件外壳。

## 1. Probe 和 XP3 Index

- [ ] 扫描 case 根目录，识别 `.xp3`、`plugin/`、`savedata/`、standalone `.tjs`。
- [ ] 读取 XP3 magic 和 `index_offset`。
- [ ] 支持 zlib index 与 raw index。
- [ ] 解析 `File/info/segm/adlr/time`。
- [ ] 输出 storage name、flags、sizes、segments、Adler-32。
- [ ] 对未知 index/segment flag 产生 diagnostic。

验收：对 3lj 样本能列出 18 个 XP3、39 个 plugin、138 个 base `.ks.scn`，不提取商业 payload。

## 2. Virtual Storage Layer

- [ ] 生成 mount order。
- [ ] 归一化 lookup key，同时保留原始 storage name。
- [ ] 记录 active source 和 shadowed sources。
- [ ] 输出 patch coverage。

验收：报告能显示 `patchAI.xp3` 覆盖 `scn.xp3` 的 138 个 `.ks.scn`。

## 3. Script Classifier

- [ ] 区分 UTF-16 TJS source 和 `TJS2100` bytecode。
- [ ] 识别 `.ks` 文本脚本。
- [ ] 识别 `.ks.scn`/PSB binary scenario。
- [ ] 识别 `.sli`、`.stage`、`.pbd`、`.toml` 等辅助格式。

验收：`patch.tjs` 归类为 bytecode，`scenario/start.ks` 归类为 KAG source，`.ks.scn` 归类为 PSB binary scenario。

## 4. KAG/TJS Boot Trace

- [ ] 初始化 TJS runtime shell。
- [ ] 实现 `Storages`、`Scripts`、`Plugins` 的统一 facade。
- [ ] 加载 `appconfig.tjs`、`default.tjs`、作品配置脚本。
- [ ] 记录 `KAGLoadScript`、plugin link、storage lookup。
- [ ] 输出 boot trace，不输出商业文本。

验收：能进入 title 或第一个稳定等待边界；失败时报告缺失 storage/plugin/source ref。

## 5. KAG Source Executor

- [ ] 解析 label、command、bracket tag、text。
- [ ] 实现 macro、jump、call、return。
- [ ] 实现 wait/trigger/timer 到 `AwaitToken`。
- [ ] 输出 TextCaptureEvent、PresentationCommand、AudioCommand、StateMachineTrace。

验收：synthetic `.ks` case 可完成 start、text、choice、jump、save/load。

## 6. `.ks.scn`/PSB

- [ ] PSB header probe。
- [ ] name tree/string/resource table 只读解析。
- [ ] 将不支持执行的 `.ks.scn` 标为 script stop diagnostic。
- [ ] 实现后映射到同一套 KAG action，不新增 public event。

验收：3lj `.ks.scn` 不再被误判成文本；支持状态在 report 中明确。

## 7. Media Bridge

- [ ] TLG/PNG/JPG image provider。
- [ ] OGG/Opus/WAV audio provider。
- [ ] `.sli` timing/loop sidecar。
- [ ] WMV/MP4 movie provider。
- [ ] text/font provider。
- [ ] unsupported plugin/media diagnostic。

验收：synthetic case 能输出 image、voice、BGM、movie command；3lj probe 能列出 codec/plugin requirement。

## 8. Snapshot 和 Release Gate

- [ ] 保存 KrKr VM snapshot ref。
- [ ] 记录 storage layer fingerprint。
- [ ] 支持 save/load round trip。
- [ ] 输出 machine-readable report。
- [ ] release gate 检查 boot、scenario、choice、text、voice、BGM、movie、save/load、shutdown。

验收：report 不含商业 payload、私有绝对路径、未授权截图或音频采样。
