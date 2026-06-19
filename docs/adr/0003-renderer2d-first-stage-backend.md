# ADR 0003: Renderer2D First Stage Backend

Status: Accepted

## Context

第一阶段需要稳定的 2D 背景、角色、UI、文本、转场和 FilterGraph 骨架。后端选择不应污染上层 Presentation、VN 或 Compat 设计。

## Decision

Renderer2D 第一阶段可使用 SDL GPU 或等价轻量后端实现 RHI。公开接口为 Astra 自有 RHI、Renderer2D、RenderGraph 和 FilterGraph DTO，不暴露 SDL 类型。

## Consequences

- 后续可替换为 bgfx、WebGPU、Vulkan/OpenGL 抽象。
- FilterGraph 和 PresentationCommand 不绑定具体后端。
- 旧游戏现代化以 layer-aware FilterGraph 为长期目标。


