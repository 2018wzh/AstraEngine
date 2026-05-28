# 构建说明

状态：Draft

## 工具链

当前验证路径使用 Windows、Ninja、vcpkg、Clang 工具链和动态链接：

- C compiler：`clang`
- C++ compiler：`clang++`
- vcpkg triplet：`x64-windows`
- 构建目录：`build`
- 引擎 Runtime 模块输出为 DLL。
- 默认 Runtime Provider 插件输出为 `build/Plugins/DefaultRuntimeProviders/Bin/win64/DefaultRuntimeProvidersPlugin.dll`。
- 示例插件输出为 `build/Plugins/ExampleRuntime/Bin/win64/ExampleRuntimePlugin.dll`。

如果 `build` 已经配置完成，直接执行：

```powershell
cmake --build build
ctest --test-dir build --output-on-failure
```

首次配置可使用：

```powershell
cmake -S . -B build -G Ninja `
  -DCMAKE_BUILD_TYPE=Debug `
  -DCMAKE_C_COMPILER=clang `
  -DCMAKE_CXX_COMPILER=clang++ `
  -DCMAKE_TOOLCHAIN_FILE=$env:VCPKG_ROOT/scripts/buildsystems/vcpkg.cmake `
  -DVCPKG_TARGET_TRIPLET=x64-windows
```

## 运行 Demo

`AstraGame` 默认扫描 `build/Plugins`。运行前需要先构建插件目标；普通 `cmake --build build` 会同时构建默认 Runtime Provider 插件和示例插件。

Headless 验证：

```powershell
.\build\Bin\AstraGame.exe --project Projects\Samples\MinimalVN --headless --route default
```

可视化 Demo：

```powershell
.\build\Bin\AstraGame.exe --project Projects\Samples\MinimalVN
```

窗口输入：

- `Space` / `Enter` / 鼠标点击：推进对白。
- `1`：选择第一个选项。
- `2`：选择第二个选项。
- `Esc`：退出。

## 依赖

`vcpkg.json` 固定第一阶段依赖集合：SDL3、FreeType、HarfBuzz、fmt、spdlog、nlohmann-json、yaml-cpp、glm、EnTT、miniaudio、Catch2、directx-dxc。

Renderer2D 第一版通过 SDL3 的 GPU renderer 路径创建渲染器。当前 Demo 资产是占位资产，Renderer2D 按 AssetId 生成稳定颜色，用来验证窗口、渲染、RuntimeCommand 和 AssetRegistry 链路。
