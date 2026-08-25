# Minori save/load E2 记录

日期：2026-08-25

本次验证覆盖 ABI v9 的 `LegacyWritableFileHostV1` 路径，使用同一 development-reuse package、mount profile 和真实 Minori PAZ 数据。记录只保留 schema、计数、状态和 hash，不保存 key、商业文本、媒体 payload 或本地路径。

## 实现边界

- 保存槽固定为 100 个，按 10×10 页面浏览；槽文件使用 `minori/saves/slot-###.bin`，目录和文件名均经过相对路径校验。
- envelope 为 `astra.emu.minori.save_slot.v1`，包含 case/package/profile identity、脚本 URI/hash 和有界 VM snapshot。
- 写入顺序为临时文件截断、bounded range write、长度确认、`AtomicReplace`。读取先 `Stat`，再按精确长度读取并校验 schema、身份、脚本 hash 与 snapshot 状态。
- `astra.launch_entry_explicit` 只选择 title/direct 入口，不属于持久化 profile identity；因此 direct 运行产生的槽可以从 title 入口恢复。其他 profile、package、脚本或 mount identity 漂移仍然阻断。

## Headless evidence

### Save

`astra.emu.headless_run_report.v3` 为 `passed`：28 fixed steps、28 presented frames、17 consumed inputs，checkpoint 为 `before_save`、`save_page`、`after_save`，diagnostic 为空。Save 页面显示 Auto Save、Quick Save、slot grid、Page0 和 Back/Next/Return 控件；视觉检查未发现裁剪、拉伸或图层残留。

### Load

使用同一 package/profile 从 title 入口打开 Load 页面并读取 slot 0，报告为 `passed`：9 fixed steps、4 presented frames，checkpoint 为 `title_initial`、`load_page`、`after_load`，diagnostic 为空。恢复后的画面回到保存时的首条消息 continuation，说明跨 session 的 VM snapshot、脚本 hash 和 host tick rebase 已生效。

## 尚未关闭的门禁

以上是 Headless E2，不是 Windows Manager E3。原版逐点 parity、完整路线上的自然 save/load、音频人工听审、CG/BGM/回想所有页面、movie fence 以及真实 Windows 输入仍需独立证据；不能由本记录推导为产品完成。

