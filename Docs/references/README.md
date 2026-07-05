# Reference Implementations

参考实现只用于提炼经验，不复制代码，也不把它们的私有架构变成 AstraEngine 的公共约束。

| Reference | Path / URL | 可借鉴点 | 不能照搬 |
| --- | --- | --- | --- |
| sena-rs | `D:/Workspace/sena-rs` | SoftPAL VM、headless diagnostic、跨平台 launcher、save snapshot、winit/wgpu/移动 host | 单 family 主循环和私有 PAL 状态不能进入 EngineCore |
| rfvp | `D:/Workspace/rfvp` | Lua-like VM、广泛实验平台、host API、platform capability、FVP case | no_std/旧主机限制不能绑死主线平台设计 |
| siglus_rs | `D:/Workspace/siglus_rs` | Siglus asset、G00/OMV/NWA、shader conversion、scene VM、平台壳 | family-specific decode/key 逻辑必须留在 EMU core |
| ethornell-rs | https://github.com/xmoezzz/ethornell-rs | BGI/Ethornell 参考 family | 不复制商业数据或绕过保护流程 |
| pfs-rs | `D:/Workspace/pfs-rs` | Artemis PF6/PF8 archive、patch chain、PFS CLI | key 和 payload 不进入 EngineCore |
| FuckGalEngine | `D:/Workspace/FuckGalEngine` | KrKr、Minori、BGI、Siglus 历史格式线索 | hook、crack、detours 和保护绕过说明不纳入 AstraEngine |

参考仓库进入 AstraEMU family adapter 的 audit checklist：probe、archive map、script VM、legacy API mapper、media decode、system UI、save/load、trace、report redaction。

Family 研究资料入口：[../emu/README.md](../emu/README.md)。
