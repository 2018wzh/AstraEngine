# KrKr Family

The Family v10 module opens a native game directory and owns one fresh child
process per session. The child loads the native owner DLL, runs the original
VM/renderer/input path, and returns its complete physical RGBA8 frame with
the authored logical stage dimensions. The SDK owns the copied texture and
validates the centered viewport; the adapter does not render the frame again.

The Host must initialize diagnostics and provide a fresh I16 stereo 48 kHz
audio sink with actual playback progress. PCM acknowledgements follow the
Host write; progress is never estimated from wall time. Control, PCM and frame
traffic use separate bounded TCP lanes. Native stdout/stderr are drained into
the Host diagnostic sink. Close cancels Host audio, stops the native owner,
waits for the child and joins every parent worker before releasing the session.

Windows runtime files must be siblings of the Family module:

```text
astra_emu_krkr.dll
astra-emu-krkr-child.exe
astra_krkr_native.dll
<native dependency DLLs>
Resources/
```

The native CMake target stages its app-local DLLs and resources. After building
the Rust library and production child, assemble them into a new directory:

```sh
python Tools/AstraEMU/package_krkr_runtime.py \
  --rust-artifacts "$RUST_ARTIFACTS" \
  --native-runtime "$NATIVE_RUNTIME" \
  --output "$KRKR_RUNTIME"
```

The script validates x64 PE headers and copies build outputs without loading
or running them. MSVC/UCRT runtime prerequisites are separate from the native
target's app-local dependency DLLs. The transport test child is never packaged.
The module is not yet registered in the Manager catalog; registration follows
native integration. Other platforms return native-unavailable explicitly.
