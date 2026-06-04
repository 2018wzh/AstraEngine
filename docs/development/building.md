# 构建

## 1. 当前构建范围

当前默认构建只包含 Phase 1 模块：

- `Astra_Core`
- `Astra_Platform`
- `Astra_ModuleRuntime`
- `Astra_PropertySystem`
- `Astra_Phase1ExampleModule`
- `AstraPhase1Smoke`
- `Astra_Phase1Tests`

旧主线如 `AstraRuntime`、`VNRuntimeServices`、`Bootstrap`、`AstraGame` 已不参与默认构建。

## 2. 依赖

当前 `vcpkg.json` 依赖为：

- `fmt`
- `spdlog`
- `nlohmann-json`
- `yaml-cpp`
- `sdl3`
- `catch2`

## 3. 配置

Windows 本地配置示例：

```powershell
cmake -S . -B build
```

如果需要明确配置类型：

```powershell
cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug
```

## 4. 编译

```powershell
cmake --build build --config Debug
```

产物位置：

- 可执行文件：`build/Bin`
- 静态库：`build/Lib`
- 示例模块：`build/Plugins/Phase1ExampleModule`

## 5. CMake 主线

顶层 [CMakeLists.txt](/E:/Documents/AstraEngine/CMakeLists.txt) 只挂接以下目录：

- `Engine/Runtime/Core`
- `Engine/Runtime/Platform`
- `Engine/Runtime/ModuleRuntime`
- `Engine/Runtime/PropertySystem`
- `Engine/Plugins/Examples/Phase1ExampleModule`
- `Engine/Programs/AstraPhase1Smoke`
- `Engine/Tests`

## 6. 当前目录约定

```text
Engine/
├─ Runtime/
│  ├─ Core/
│  ├─ Platform/
│  ├─ ModuleRuntime/
│  └─ PropertySystem/
├─ Plugins/
│  └─ Examples/
│     └─ Phase1ExampleModule/
├─ Programs/
│  └─ AstraPhase1Smoke/
└─ Tests/
```

## 7. 非目标

当前构建文档不覆盖：

- Editor
- VN demo
- Asset cook pipeline
- Renderer2D / Audio / Text
- Legacy runtime compatibility modules
