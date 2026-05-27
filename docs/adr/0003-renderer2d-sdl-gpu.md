# ADR 0003: SDL_GPU for First-Phase Renderer2D Backend

Status: Accepted

## Context

Renderer2D needs an initial backend that can support VN-focused 2D rendering, texture uploads, batching, blending, transitions, and future post-processing. Candidate backends include SDL_GPU, bgfx, WebGPU, and OpenGL.

## Decision

The first-phase Renderer2D backend is SDL_GPU, aligned with the SDL3 platform baseline. Phase 0 records this design decision only; it does not implement RHI or Renderer2D targets.

## Consequences

- The first RHI implementation should model SDL_GPU concepts without exposing SDL types above the platform and rendering backend boundary.
- Renderer2D must remain independent of VN DSL, Editor, and AI modules.
- If SDL_GPU cannot satisfy required rendering features, a later ADR must document the replacement or additional backend.
