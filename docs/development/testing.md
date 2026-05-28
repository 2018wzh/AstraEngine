# 测试说明

状态：Draft

## 命令

```powershell
cmake --build build
ctest --test-dir build --output-on-failure
.\build\Bin\AstraGame.exe --project Projects\Samples\MinimalVN --headless --route default
.\build\Bin\AstraGame.exe --project Projects\Samples\MinimalVN --headless --route second
```

## 当前覆盖

`Astra_RuntimeTests` 覆盖：

- AssetId 语法。
- AssetRegistry sidecar 扫描。
- ExtensionRegistry 重复注册诊断。
- VN Property System JSON Schema generation。
- ModuleManager discovery、真实动态库加载、生命周期和扩展注册。
- DefaultRuntimeProviders 插件发现和 Platform / Renderer / Audio / ProjectContent Provider 注册。
- AstraRuntimeSession headless 执行、选择分支、变量变化和 save/restore。

## 手动验收

可视化 Demo 需要人工确认窗口渲染、输入和关闭行为：

```powershell
.\build\Bin\AstraGame.exe --project Projects\Samples\MinimalVN
```

当前 Renderer2D 使用占位色块展示背景、立绘、对白框和选择菜单；这验证 Runtime 到渲染层的链路，不代表最终素材加载和文本排版品质。
