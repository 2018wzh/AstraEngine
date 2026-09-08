# Independent Family API v1

`astra-emu-family-api` is the only contract dependency required by a family
plugin. It has no dependency on `RuntimeWorld`, package/save containers, VFS
services, renderer/audio backends, or product UI.

The dynamic root module exposes one `FamilyModule` ABI-stable vtable. Its six
operations are synchronous and use the same DTOs as static providers:

```rust
fn descriptor() -> FfiFamilyResult<FamilyDescriptor>;
fn probe(ProbeRequest) -> FfiFamilyResult<ROption<ProbeReport>>;
fn open(OpenRequest) -> FfiFamilyResult<OpenResponse>;
fn advance(AdvanceRequest) -> FfiFamilyResult<AdvanceResponse>;
fn frame(SessionRequest, FrameConsumerBox) -> FfiFamilyResult<()>;
fn close(SessionRequest) -> FfiFamilyResult<()>;
```

`ROption<ProbeReport>` represents a normal no-match. `game_id` is an opaque
provider-defined UTF-8 identifier; it is not assumed to be an ASCII filename.
Descriptor validation requires `CpuFrame`, rejects duplicate capabilities and
formats, and rejects the historical ABI fingerprint. `PcmAudio` is optional,
but a family that declares it must receive a host audio sink and return a
format from `open`; a returned format without the capability is also rejected.

The host supplies one `game_path` and the initial `WindowState`. The family
owns all script execution, file reads, decoding, mixing, rendering, and native
save/load. `FamilyEvent` preserves ordered keyboard, pointer, wheel, text,
focus, resize, visibility, suspension, and close events. `KeyCode` includes
both Control keys so a host can bind Ctrl fast-forward without a string or
platform-specific key ABI. `PointerMove` uses game-frame pixel coordinates
after the host applies letterbox/scaling (output-scale coordinates are mapped
back to the original frame); `WindowResized` uses physical client pixels; and
wheel deltas use OS detent units, including fractional high-resolution values.
`AdvanceRequest::elapsed_ns` accepts every `u64`, including zero; it is elapsed
input to the family rather than a host fixed step. Native save/load and new
game remain inside the family, while the optional text service receives typed
reset signals for its session cache.

When audio is enabled, the family returns one `PcmFormatSpec` and first calls
`AudioSink::configure` with that exact format during `open`. Only after this
handshake may an audio worker call `write`. The sink is a bounded,
cancellable queue: `is_cancelled` and `cancel` let `close` interrupt a worker
before session teardown. A device callback never enters the family. `PcmChunk`
validates the selected format, channel alignment, sample bound, and finite
`F32` values.

`FrameView<'a>` contains only a CPU `RSlice<'a, u8>` and `FrameInfo`. The sole
format is explicit opaque RGBA8 sRGB. `FrameConsumer::accept` is synchronous;
the lifetime is tied to that call and cannot be manufactured as `'static`.
The host copies the required `stride * height` bytes before returning from the
callback. No GPU or native device handle crosses the boundary.

Text replacement is an optional `TextReplacementService` with typed reset,
submit, poll, and cancel methods. Poll returns exactly `Pending`,
`Ready(response)`, `Cancelled`, or `Failed(error)`. The service owns its
session context cache, bounded to the most recent eight entries and 6000
characters. The family request contains only the current source and optional
speaker/ruby fields. Family text replacement remains optional; FVP does not
advertise or invoke it in this API revision.

The ABI identity is `astra.emu.independent_family_abi.v1`, with module name
`astra-emu-independent-family`. Historical family ABI binaries are rejected;
no compatibility adapter is defined.
