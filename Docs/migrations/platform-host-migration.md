# Platform Host Realignment Migration

本计划记录已完成的 `Stage 2` Windows 平台实现（wgpu Surface 创建、WASAPI 声音输出、WMF 视频解码、以及 Windows Gate 校验）向 `AstraPlatform` 统一不透明 Token 与 `PlatformHost` 接口收束的迁移路线。

**本迁移不仅适用于 AstraEditor，同样强制约束 AstraEngine 核心（EngineCore）与所有公共 Runtime 模块（包括 `astra-runtime`、`astra-media-core` 等）。**

设计页可以覆盖完整未来架构；本迁移页仅针对已有实现的重构对齐。

## 现有实现入口

- `Engine/Source/Runtime/astra-platform-windows/`：包含 Windows host adapter 的主入口、winit 隐藏窗口句柄获取、WASAPI `audio.wasapi` 实现、以及 WMF 视频解码 `decode.wmf.video_first_frame` 逻辑。
- `Engine/Source/Runtime/astra-runtime/`：公共 Runtime 模块，其原有部分测试与运行逻辑中直接调用了平台的窗口和音频底层抽象。
- `Docs/platforms/desktop.md`：记录已完成 of `S2-WINDOWS-HOST-01`、`S2-WINDOWS-WMF-01`、`S2-WINDOWS-GATE-01` 验收规范与证据口径。
- `Docs/implementation/platform-host.md`：定义 `PlatformHost` trait 职责。
- `Editor/Source/Bridge/astra-editor-bridge/src/pie.rs`（Stage 4 设计）：PIE Viewport 原有直接操作底层 `HWND` / `RawWindowHandle` 的嵌入方式。

## 目标设计

为防止底层原生句柄（如 `HWND`、`NSView*`、`XID` 等）与平台特定行为在核心层泄露，整个引擎与编辑器集成必须符合以下对齐原则：

1. **底层句柄完全封装**：
   `astra-editor-bridge`、AstraEngine 核心库及所有公共 Runtime 模块一律只传递不透明的 `win_id`（`u64` 整数）与通用的 `SurfaceRequest`。
2. **Surface 凭证由 Token 表达**：
   `PlatformHost::create_surface` 返回的 `SurfaceToken` 承载平台相关的 Surface 凭证。核心引擎（EngineCore）与编辑器只持有该 Token，不接触任何 `win32` 或 `raw-window-handle` 特有的 C 结构体或指针。
3. **Wayland 等环境自动透明降级**：
   是否降级为离屏 Texture 共享，完全在 `astra-platform-linux` 等底层实现中根据 QPA / windowing 协议类型自动检测和处理，返回 `SurfaceToken::TextureShared`。核心 Runtime 仅根据 SurfaceToken 类型选择渲染流程，不参与任何低层窗口系统的环境检测与判定。

---

## 分步迁移

### 1. 重构 `PlatformHost` 接口定义 (`astra-platform`)

修改 `astra-platform` 的 `PlatformHost` 声明以接收通用 `SurfaceRequest` 并输出不透明 `SurfaceToken`：

```rust
// Engine/Source/Runtime/astra-platform/src/lib.rs
pub struct SurfaceRequest {
    pub window_handle: u64,          // 平台不透明窗口句柄
    pub size_width:    u32,
    pub size_height:   u32,
    pub allow_fallback: bool,        // 是否允许 texture 共享降级
}

pub enum SurfaceToken {
    /// 原生子窗口嵌入（Windows HWND, macOS NSView, Linux X11）
    NativeEmbed,
    /// 降级离屏渲染，输出 GPU 共享材质句柄 (例如 Wayland 模式下)
    TextureShared {
        texture_id: u64,
    },
}
```

### 2. 重构 Windows host 实现 (`astra-platform-windows`)

更新 `astra-platform-windows` 中的 `PlatformHost` 实现，将原有的 raw handle 映射收拢在 Rust 内部：

```rust
// Engine/Source/Runtime/astra-platform-windows/src/lib.rs
impl PlatformHost for WindowsPlatformHost {
    fn create_surface(&mut self, request: SurfaceRequest) -> PlatformResult<SurfaceToken> {
        // 1. 在平台实现内部，安全地将 u64 的 window_handle 转换为 Windows HWND
        let hwnd = request.window_handle as HWND;
        
        // 2. 利用 raw_window_handle 将 HWND 转换为 wgpu 兼容句柄（不泄漏到外部）
        let raw_handle = build_win32_handle(hwnd);
        
        // 3. 构建 wgpu Surface 并存入内部上下文，返回不透明 SurfaceToken::NativeEmbed
        self.bind_surface_to_viewport(raw_handle, request.size_width, request.size_height)?;
        Ok(SurfaceToken::NativeEmbed)
    }
}
```

### 3. 重构 EngineCore 渲染绑定管线 (`astra-runtime` / `astra-media-core`)

- 修改核心 Runtime 中 wgpu Surface 的初始化逻辑，使其仅接受由 `PlatformHost::create_surface` 返回的 `SurfaceToken`。
- 对于 `SurfaceToken::NativeEmbed`，Runtime 模块通过内部转换（仅存在于 `astra-platform` 实现层）执行标准的 Present 链；对于 `SurfaceToken::TextureShared`，Runtime 降级到 FBO 离屏渲染并将纹理句柄回传，不参与任何原生的平台窗口交互。

### 4. 重构已完成的 Windows Headless/Smoke 测试证据

原有的 Windows Required Gate 依赖隐藏窗口的 `renderer.wgpu_surface` 创建证明。需重构测试证据生成方式：
- 隐藏窗口的 winId 同样通过 `create_surface` 请求进行关联，由 Windows platform 适配器内部捕获并输出 `PlatformCapabilityReport` 中的 evidence 键值。
- 测试代码中剔除直接暴露 `HWND` 的 debug断言，改为断言 `SurfaceToken` 返回为 `NativeEmbed` 且 status 为 `pass`。

### 5. 重构编译与 CI matrix 对齐

更新 `github workflows` 配置文件，在三平台 matrix 下执行测试时，`astra-platform-windows` 仅在 Windows runner 下激活。Linux 与 macOS runner 独立运行其各自的 capability report 模板检查。

---

## 验收命令

```bash
# 执行文档健康检查
python Tools/check_docs.py

# 平台抽象 Realignment 单元测试
cargo test -p astra-platform windows_surface_token_create
cargo test -p astra-platform-windows wasapi_output_token
cargo test -p astra-runtime runtime_surface_token_binding
```

---

## 不得修改项

- **禁止泄漏句柄**：`astra-platform` 的 API 绝对不能以 `raw_window_handle::RawWindowHandle`、`*mut c_void` 或底层窗口 ID 直接作为公共接口参数暴露给外部或公共核心 Crate。
- **禁止在核心引擎中进行平台分支检测**：公共 Runtime 模块（`astra-runtime`、`astra-media-core`）绝对不能包含针对特定窗口系统（如 X11 vs Wayland，或 Windows vs Linux）的环境检测代码，所有降级或平台切换行为必须在 `AstraPlatform` 的适配器底层完全隐藏。
- **保持 Save/Decode 不变**：WASAPI 音频和 WMF 视频解码已完成部分的业务数据流无需调整，仅限制其系统依赖句柄的生命周期与 `PlatformHost` 对齐。
