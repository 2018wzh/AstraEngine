# Reference Implementations

参考实现只用于提炼经验，不复制代码，也不把它们的私有架构变成 AstraEngine 的公共约束。

| Reference | 来源 | 可借鉴点 | 不能照搬 |
| --- | --- | --- | --- |
| sena-rs | 本地合法参考仓库 | SoftPAL VM、headless diagnostic、跨平台 launcher、save snapshot、winit/wgpu/移动 host | 单 family 主循环和私有 PAL 状态不能进入 EngineCore |
| rfvp | 本地合法参考仓库 | Lua-like VM、广泛实验平台、host API、platform capability、FVP case | no_std/旧主机限制不能绑死主线平台设计 |
| siglus_rs | 本地合法参考仓库 | Siglus asset、G00/OMV/NWA、shader conversion、scene VM、平台壳 | family-specific decode/key 逻辑必须留在 EMU family plugin |
| ethornell-rs | https://github.com/xmoezzz/ethornell-rs | BGI/Ethornell 参考 family | 不复制商业数据或绕过保护流程 |
| pfs-rs | 本地合法参考仓库 | Artemis PF6/PF8 archive、patch chain、PFS CLI | key 和 payload 不进入 EngineCore |
| FuckGalEngine | 本地合法参考仓库 | KrKr、Minori、BGI、Siglus 历史格式线索 | hook、crack、detours 和保护绕过说明不纳入 AstraEngine |

参考仓库进入 AstraEMU family plugin 的 audit checklist：probe、archive map、script VM、legacy API mapper、media decode、system UI、save/load、trace、report redaction。

Family 研究资料入口：[../emu/README.md](../emu/README.md)。
