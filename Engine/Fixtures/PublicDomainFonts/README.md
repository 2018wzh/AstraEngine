# Open Font Fixtures

这里的字体只用于 hermetic shaping、fallback、raster、Package/VFS 和视觉回归。三个字体均来自 `google/fonts` 的同一固定 revision `ec0464b978de222073645d6d3366f3fdf03376d8`，使用 SIL Open Font License 1.1；对应许可原文与字体一起提交。

- `NotoSansSC-Variable.ttf`：CJK、日文假名和东亚标点。
- `NotoSansArabic-Variable.ttf`：Arabic shaping、RTL 与组合字符。
- `NotoEmoji-Variable.ttf`：monochrome emoji、variation sequence 与复杂 cluster。

`manifest.json` 固定 source URL、revision、byte size、SHA-256、family 和测试 coverage。测试只能从 manifest 声明的文件构造 package font database，不能退回系统字体。更新字体时必须同时更新 hash、许可、layout/glyph golden 和 drift evidence。
