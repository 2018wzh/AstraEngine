# UI Component Plugin Contract

本契约定义作品专属 UI component 的跨 ABI 边界。通用 plugin ABI 仍负责 artifact identity、load/unload 和签名 helper；UI component 类型只由 `astra-ui-plugin-abi` 暴露。

## Descriptor 与 slot

provider descriptor 声明 provider id、schema、component types、input/output model schema、capability、target、profile eligibility、state limit、effect limit、artifact fingerprint 和 signer。`.astra` 的 `ui_component` 必须静态声明 typed slot：

```text
slot id:route_chart
  provider:project.route_chart
  component_type:route-chart
  max_instances:1
  instance_key:$model.graph_id
```

运行时只能在该 slot 内按允许的 provider/type/count 动态 mount。缺声明、类型不匹配、instance key 不稳定或超过数量均阻断。

## Lifecycle

ABI 只传 bounded serialized DTO：

```text
provider_create / provider_destroy
session_open / session_close
component_mount / component_update
component_event / component_snapshot
component_restore / component_unmount
```

host 返回 logical component/session id，不返回 Rust pointer、Yakui node、callback、wgpu handle、window handle 或 filesystem object。effect 必须是已注册、schema-bound 的 UI action、semantic patch、render primitive 或 repaint request。

## Trust 与 capability

Windows dylib 必须通过 Ed25519 signature、signer allowlist、engine/rustc/feature/provider fingerprint 和 canonical manifest hash。进程内 dylib 是完全受信代码；host capability 不能阻止它绕过 host 直接调用 OS，release owner 必须把签名者视为本机代码发布者。

Web component 以 Rust schema 为真源生成 WIT adapter。Cook 使用锁定且校验 hash 的 jco 生成 ES module/core wasm，bundle manifest 记录输入 component、WIT、jco、输出 module/wasm 的 hash。浏览器 host 只加载验证后的 bundle artifact。

host API capability 限于 IME、clipboard read/write 和 open-external-URL。每项由 target/profile allowlist 授权，clipboard/open URL 还需要用户手势和平台权限证据。没有任意 host 文件、socket 或 process API。

## Bounds 与失败

固定 hard limit：depth 32、4096 nodes/view、1024 instances/view、4 MiB/DTO、1 MiB/provider-session state、256 effects/call、64 MiB Web memory。release profile 必须额外给出 time/fuel budget。

panic、trap、非法 schema、超限、timeout、signature mismatch、restore mismatch 或 capability violation 会终止整个 UI session并输出唯一根因 diagnostic。host 不生成替代组件，不回退到内建组件，也不换另一个 provider。

## State 与隐私

component snapshot 只用于同一 UI session 的 rebuild/restore，且必须匹配 provider/schema/fingerprint。它不进入 VN save、route state 或 replay authority。报告只写 schema、provider、component type、stable instance key hash、artifact hash、计数、耗时和 diagnostic code，不写用户文本、clipboard、商业 payload、源路径或整体 DTO。

## Gate

Migration 12 使用签名 Windows dylib fixture 和签名 Web component fixture 验证注册、mount/update/event/snapshot/restore/unmount、bounds、permission、redaction 和失败终止。fixture 只能证明 ABI，不可作为正式 Message、Choice、Save、Backlog 等产品页的运行依赖。
