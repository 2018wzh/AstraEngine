# Phase 1 Smoke

## 1. 目标

`AstraPhase1Smoke` 用于验证当前最小宿主链路：

- 平台服务初始化
- 服务注册
- 插件发现
- 插件加载
- 扩展注册
- 插件停用与卸载

## 2. 相关文件

- 程序入口：[Engine/Programs/AstraPhase1Smoke/Private/main.cpp](/E:/Documents/AstraEngine/Engine/Programs/AstraPhase1Smoke/Private/main.cpp)
- 示例插件：[Engine/Plugins/Examples/Phase1ExampleModule/Private/Phase1ExampleModule.cpp](/E:/Documents/AstraEngine/Engine/Plugins/Examples/Phase1ExampleModule/Private/Phase1ExampleModule.cpp)
- 插件描述：[Engine/Plugins/Examples/Phase1ExampleModule/Phase1ExampleModule.plugin.yaml](/E:/Documents/AstraEngine/Engine/Plugins/Examples/Phase1ExampleModule/Phase1ExampleModule.plugin.yaml)

## 3. 运行

```powershell
.\build\Bin\AstraPhase1Smoke.exe
```

也可以显式传入插件目录：

```powershell
.\build\Bin\AstraPhase1Smoke.exe .\build\Plugins\Phase1ExampleModule
```

## 4. 成功条件

输出应显示：

- 已加载 1 个模块
- 已注册 `phase1_example.service_extension`
- 已注册 `phase1_example.property_type_provider`

## 5. 作用边界

这个 smoke 不是 gameplay demo，也不是 VN demo。它只证明：

- 宿主和模块 ABI 能对接
- descriptor 能发现与校验
- capability / permission 边界有效
- `ServiceRegistry` / `ExtensionRegistry` 主线可用
