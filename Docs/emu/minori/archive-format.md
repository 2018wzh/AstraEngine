# Minori Archive Format

Minori 游戏以多个 `.paz` archive 分区保存资源。`夏空のペルセウス` 使用：

| Archive | 观测大小 | 预期内容 |
| --- | ---: | --- |
| `scr.paz` | 1914452 | 脚本 `.sc`、流程和文本引用 |
| `st.paz` | 852999948 | 背景、立绘和事件图 |
| `sys.paz` | 32418204 | UI、字体、系统图 |
| `se.paz` | 13554180 | SE |
| `voice.paz` | 326048276 | voice |
| `mov.paz` | 0 | 当前样本未使用 movie payload |

## 解析模型

PAZ reader 采用三段式：

1. 读取 header，识别 archive 类型、entry 数、TOC offset 和 TOC size。
2. 使用外部 key config 解开 TOC。key 来源只能是命令行、用户配置或 case manifest。
3. 对 entry payload 执行 per-file transform、zlib inflate 或 raw passthrough。

## Lookup

Core 按 archive role 建立 VFS：

```text
script -> scr.paz
stage/image -> st.paz
system -> sys.paz
se -> se.paz
voice -> voice.paz
movie -> mov.paz 或 loose file
patch -> *.mys / *.acr / 外部只读 mount
```

查找必须大小写不敏感，但 trace 保留原始 entry name。多个 archive 命中时，patch mount 优先，之后按 role 固定顺序。

## 安全规则

PAZ key 不写入源码，不写入文档正文。工具和 core 遇到缺 key 时返回 `NeedsUserKey` diagnostic，不能尝试从 exe、补丁 DLL 或 hook 材料自动提取。
