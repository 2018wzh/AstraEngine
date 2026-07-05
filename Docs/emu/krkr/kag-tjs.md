# KAG/TJS Boot

KrKr 的启动不是“打开一个 `.ks` 文件”这么简单。TJS 先搭系统对象、配置、插件和 KAG runtime，KAG 再执行 title/main scenario。

## Boot Chain

样本和参考代码给出的最小链路：

1. 读取 XP3 和 standalone patch script，建立 virtual storage。
2. 初始化 TJS global、`System`、`Storages`、`Scripts`、`Plugins`。
3. 执行配置脚本，例如 `appconfig.tjs`、`default.tjs`、`yuzu_default.tjs`、`custom.tjs`。
4. 绑定 `SystemConfig`、`kag`、`tf`、`sf` 等运行时状态。
5. 加载 KAG 支持脚本和 UI 脚本。样本 source 中可见 `KAGLoadScript("yuzu_default.tjs")`。
6. 进入 title 或 first scenario。样本 bytecode 常量池中能看到 `first.ks`、`title.ks`、`start.ks`；archive 中有 `scenario/start.ks`。

AstraEMU 的 boot probe 要输出每一步是否找到 storage、是否 source/bytecode、是否执行或跳过。不能只报“boot failed”。

## TJS Runtime 角色

TJS 负责：

- system config 和作品 config。
- plugin link，例如 `toml.dll` 这类格式插件。
- KAG class、window/menu/action glue。
- save/load、backlog、voice replay、scenario chart、system menu。
- 路由和多语言配置。

Lua 5.4 是 AstraEMU 的 patch/decode runtime，不替代 TJS。KrKr 兼容要保留 TJS 语义，否则 `SystemConfig`、`kag`、`tf/sf`、plugin API 和旧菜单逻辑都会失真。

## KAG Conductor

`Conductor.tjs` 展示了 KAG 执行的核心形态：

- conductor 有 `mStop`、`mRun`、`mWait` 状态。
- tag name 映射到 handler。
- `wait(until)` 暂停执行，`trigger(name)` 恢复。
- timeout 用 timer 触发。
- unknown tag 进入错误或 fallback handler。

AstraEMU 应把这套模型落到 deterministic tick：

```text
KagStep -> TagDispatch -> Action
Action::Wait -> AwaitToken
input/timer/media completion -> ordered RuntimeEvent at tick boundary
```

不要让 Tokio task completion order 直接改变 KAG 状态。

## Storage 名和路径

KrKr 脚本经常用无扩展名、相对 storage、或者带目录的 storage。resolver 需要支持：

- `start.ks` 与 `scenario/start.ks` 的上下文解析。
- `KAGLoadScript("x.tjs")` 的 TJS script 查找。
- image/audio/movie 不带扩展名时的 provider search。
- patch layer 覆盖后的同名 storage。

查找失败时，diagnostic 至少包含 requested storage、caller storage、当前 layer order 和候选扩展名。

## 最小 Trace

boot trace 不记录商业台词，记录结构事件：

```text
BootBegin
MountArchive(name, rank, entry_count)
LoadTjs(storage, kind=source|bytecode)
LinkPlugin(name, capability)
LoadKag(storage)
DispatchTag(name, source_ref)
Wait(token, reason)
BootReady(mode=title|scenario)
```

这样 release gate 可以判断 KrKr core 是否真的走过 boot 链路，而不是只完成文件扫描。
