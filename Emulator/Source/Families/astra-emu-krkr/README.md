# KrKr Family

The Family v11 module opens a native game directory and owns one fresh child
process per session. The child loads the native owner DLL, runs the original
VM/renderer/input path, and returns its complete physical RGBA8 frame with
the authored logical stage dimensions. The SDK owns the copied texture and
validates the centered viewport; the adapter does not render the frame again.

Pass the original native game directory as `game_path`; it is not an Astra
package or an individual XP3 archive. Probe checks for a native XP3 header and
does not establish that the game's scripts or plugins can run. The Family
uses the original storage, VM and save handling inside its child process.
For a loose game it launches that directory's `startup.tjs`; for the
conventional archive layout it launches `data.xp3` through the same native
selector path. A directory lacking both entries fails explicitly.

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
  --output "$KRKR_RUNTIME" \
  --runtime-file "$MSVC_RUNTIME_DIR/msvcp140.dll" \
  --runtime-file "$MSVC_RUNTIME_DIR/vcruntime140.dll" \
  --runtime-file "$MSVC_RUNTIME_DIR/vcruntime140_1.dll" \
  --runtime-file "$MSVC_OPENMP_DIR/vcomp140.dll"
```

The script validates x64 PE headers and copies build outputs without loading
or running them. The three MSVC runtime DLLs and the OpenMP runtime used by
OpenCV must accompany the production child and native owner; copies beside
the Manager cannot satisfy the child loader. Windows supplies UCRT. The
transport test child is never packaged. A Windows development bundle can
discover the Family from `cores/`; native integration remains in progress.
Other platforms return native-unavailable explicitly.

当前 Windows 开发包已完成固定依赖安装，以及核心 OFF/ON、Rust Family/child
的 Release 编译和链接。已修复同目录 VC/OpenMP runtime 打包、Winsock 接入
套接字的阻塞模式，以及最终回复发送前关闭接收方向造成的清理等待；九项
Windows 传输回归与三项原生回调回归通过，all-target Clippy 和生产构建通过。
嵌入启动补齐 Cocos 的第三次零 elapsed tick，并复用原生归档入口。Sandbox
实际使用 NVIDIA OpenGL 4.6，已进入样本脚本，但启动阶段的原生模态提示框
被当前嵌入接口明确拒绝。游戏最终帧、输入、退出重开、BPG 解码和可听音频
尚未通过，不能计为可玩核心；没有跳过提示框或修改样本脚本。

Horizontal wheel input returns `ASTRA_EMU_KRKR_HORIZONTAL_WHEEL_UNSUPPORTED`.
Embedded native modal dialogs fail through the private native ABI and surface
as `ASTRA_EMU_KRKR_BOOT` or `ASTRA_EMU_KRKR_ADVANCE`; they do not return a
separate modal-dialog error code.
