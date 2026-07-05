# Web Platform

Web 目标是 wasm32 + WebGPU/WebGL fallback + WebCodecs。Web 平台必须遵守浏览器 sandbox，不提供任意本地文件系统或后台线程假设。

## Boundaries

- Package 通过 File API、Origin Private File System 或 HTTP range source 加载。
- DecodeProvider 优先 WebCodecs；不可用时使用受限软件 fallback。
- Audio 使用 WebAudio provider。
- Runtime deterministic tick 不依赖 requestAnimationFrame 时间抖动。

## Testing

Web gate 覆盖 browser launch、WASM package load、input scenario、save persistence、audio unlock、decode capability report 和 provider-free replay。
