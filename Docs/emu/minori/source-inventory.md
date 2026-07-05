# Minori Source Inventory

## 本地样本

| 路径 | 用途 |
| --- | --- |
| `E:\Games\夏空的英仙座（夏空のペルセウス）` | 当前 Minori game case |
| `D:\Workspace\FuckGalEngine\Minori` | 历史翻译/封包工具参考，只提炼格式事实 |

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
汉化补丁单独备份/
```

`mov.paz` 当前大小为 0，movie 支持仍要在 core 中保留，因为同 family 其他样本可能使用独立 movie archive。

## 参考文件

| 文件 | 可借鉴点 | 不纳入内容 |
| --- | --- | --- |
| `D:\Workspace\FuckGalEngine\Minori\sc_text_out.py` | `.sc` 文本提取规则线索 | 商业脚本文本 |
| `D:\Workspace\FuckGalEngine\Minori\sc_text_in.py` | 文本回写字段顺序线索 | patch 注入流程 |
| `D:\Workspace\FuckGalEngine\Minori\Minori\fuckpaz\main.cpp` | PAZ TOC 和 payload 处理线索 | 内置 key 或保护绕过 |
| `D:\Workspace\FuckGalEngine\Minori\Minori\MinoriPatch\scriptparser.cpp` | `.sc` command/text 解析线索 | hook 和 exe 修改 |

## 资料可信度

PAZ 与 `.sc` 资料来自本地样本和历史工具交叉验证。实际 core 实现前必须用 `Tools/AstraEMU/minori_probe.py`、`minori_paz.py` 和手工小样本确认当前游戏的 header、TOC、压缩和脚本字段。
