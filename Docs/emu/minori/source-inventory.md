# Minori Source Inventory

## 本地样本

| 路径 | 用途 |
| --- | --- |
| `<minori-case-root>` | 当前 Minori game case |
| 历史 Minori 工具参考 | 历史翻译/封包工具参考，只提炼格式事实 |

游戏根目录观测：

```text
夏空的英仙座.exe
perseus.exe
perseus.chm
perseus_chs.mys
scr.paz
st.paz
sys.paz
se.paz
voice.paz
mov.paz
bg.paz
bg.pazA ... bg.pazJ
bgm.paz
汉化补丁单独备份/
```

当前样本有八个非空逻辑 archive、18 个物理文件。`bg` 由主包和 A–J 十个连续分卷组成；`bgm` 是独立主包。文件名、计数和大小可以进入脱敏研究记录，key、payload、完整脚本文本与导出内容只保存在 ignored 私有目录。

## 参考文件

| 文件 | 可借鉴点 | 不纳入内容 |
| --- | --- | --- |
| `sc_text_out.py` | `.sc` 文本提取规则线索 | 商业脚本文本 |
| `sc_text_in.py` | 文本回写字段顺序线索 | patch 注入流程 |
| `fuckpaz/main.cpp` | PAZ TOC 和 payload 处理线索 | 内置 key 或保护绕过 |
| `scriptparser.cpp` | `.sc` command/text 解析线索 | hook 和 exe 修改 |

## 资料可信度

PAZ 结论按 GARbro contract、本地样本观察和推测分开记录。八个真实 index、14502 个 entry 和 18 个物理文件已完成 decoded full verify，用于复核 header、TOC、分卷、entry bounds、archive role 和首尾随机复读。该轮明确关闭 cache，因此不构成 cache hit 证据。89 个 `.sc` 的全包 census 已确认 CP932 行式结构与 command token。未确认的 operand 语义不从文件名或文本形态猜测。
