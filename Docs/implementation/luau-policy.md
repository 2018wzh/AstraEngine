# Luau 与宿主接口

Luau 是可信游戏脚本。当前产品边界见[重构契约](../contracts/rebuild.md)，不再要求所有状态修改经过 FSM action，也不把旧 sandbox、通用回放或未来 RPG/AI provider 作为主路径。

## 当前实现

`astra-policy` 提供值、预算和 VM 公共能力；`astra-vn-policy` 绑定 VN 查询、命令、动作和显式状态。当前 VM 初始化仍使用 `create_sandboxed_lua` 限制内置入口，这描述现有 bootstrap，不表示宿主文件/网络能力被产品契约禁止，也不构成不可信脚本安全边界。

`PolicyExecutionBudget` 控制指令、内存、输出和状态深度。预算失败、非法值与宿主失败必须返回诊断，不能转为成功。保存 `PolicySnapshotValue`、`VnPolicyState` 和 continuation 数据，不保存 Function、Thread、UserData、闭包、协程栈或原生句柄。

## 动作与异步 IO

包内注册源码通过 `astra.command.register` 声明动作及字段 schema。注册阶段结束后不能继续注册；动作 handler 使用同一 VM 的运行 API，并以 `PolicyActionContinuation` 显式保存进度。

已实现的 Lua IO 接口是 `astra.host.read_text(key, resource)`、`cancel(key)` 和 `release(key)`：

- `PolicyIoCapabilities` 由嵌入宿主授予具体 VFS 资源与字节上限，包或脚本不能自行增加权限。
- `PolicyIoHost::read_text` 立即返回可取消 Future；请求、状态与失败原因由 `PolicyIoState` 持有。
- Lua 读取 pending/ready/failed 状态，不阻塞 VM 等待外部工作；有 pending 请求时，先取消再 release。
- 成功求值后才派发外部请求。关闭、读档或替换作用域使旧任务失效，迟到结果不能覆盖新会话。

宿主可以按可信脚本需求提供文件或网络实现，但现有上述入口不代表 Packaged Player 已具备通用磁盘/网络 adapter。资源授权、宿主实现和实际产品接入状态须分别记录，不能用 fixture host 冒充平台能力。预览 seek 不重复外部副作用。

## 查询与包数据

VN 查询和命令绑定使用真实 `PolicyQueryContext` 与包内源码/manifest。缺键、未知命令、损坏缓存或不匹配身份明确失败。现有包的 source cache 和内容校验继续由代码负责；不另建一套 speculative policy/runtime provider、审计 ledger 或审批报告。

接口真源位于 `astra-vn-policy/src/{actions,host_io,luau_actions,luau_io}.rs`；调用与作用域由 typed `VnSession` 管理。公共 VM/值类型见 `astra-policy`，当前状态见[实施计划](../status/implementation-plan.md)。

## 定向验证

```sh
cargo test -p astra-policy
cargo test -p astra-vn-policy --test luau_actions
cargo test -p astra-vn-policy --test luau_mutation
```

这些是普通逻辑与宿主边界回归，不要求启动 Headless；文件/网络、声音和画面能力仍需对应真实宿主验证。
