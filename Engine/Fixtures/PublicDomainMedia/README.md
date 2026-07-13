# Public Domain Media Fixtures

These fixtures come from MDN interactive examples and are covered by the MDN `interactive-examples` repository `CC0-1.0` license.

Source pages:

- `flower.mp4` and `flower.webm`: https://interactive-examples.mdn.mozilla.net/pages/tabbed/video.html
- `t-rex-roar.mp3`: https://interactive-examples.mdn.mozilla.net/pages/tabbed/audio.html
- License: https://github.com/mdn/interactive-examples/blob/main/LICENSE

`flower-roar.mp4` 是只供 hermetic A/V sync、真实音频 meter 和恢复测试使用的派生 fixture：视频流直接取自 `flower.mp4`，音频由 `t-rex-roar.mp3` 转为 AAC。两个输入均在同一份 `CC0-1.0` 来源清单内；派生文件的输入 identity、编码方式、hash 和预期 metadata 固定在 `manifest.json`。

`manifest.json` records source URLs, byte sizes, hashes and expected media metadata. Do not replace these files with commercial payloads.
