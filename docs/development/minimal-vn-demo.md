# MinimalVN Demo

状态：Draft

样例项目位于 `Projects/Samples/MinimalVN`。

运行 Demo 前需要先构建 `DefaultRuntimeProvidersPlugin`。`AstraGame` 通过 `build/Plugins` 下的 descriptor 加载 ProjectContent、Platform、Renderer 和 Audio Provider；headless route 仍会加载 Provider 插件，但不会创建 Renderer2D 或 AudioCore 实例。

## 项目结构

```text
MinimalVN.vnproj.yaml
Config/DefaultGame.yaml
Content/Scripts/main.astra
Content/Backgrounds/*.asset.yaml
Content/Characters/*.asset.yaml
Content/Audio/*.wav
Content/Audio/*.asset.yaml
```

## 脚本语法

第一版 `.astra` 语法支持：

```text
scene school_rooftop:
  bg "native:/Backgrounds/RooftopEvening" with fade(0.8)
  show "native:/Characters/Alice/Normal" at center
  play bgm "native:/Audio/QuietWind"
  alice "You finally came."
  choice:
    "Apologize":
      set affection.alice += 1
      goto apologize_branch
```

支持指令：

- `scene`
- `bg`
- `show`
- `play bgm`
- `play sfx`
- `speaker "text"`
- `choice`
- `set +=`
- `goto`

`agent` block 会输出 unsupported diagnostic，暂不进入 Runtime 0.1 执行路径。

## 验证输出

默认 headless route 输出 RuntimeCommand log：

```text
ShowBackground native:/Backgrounds/RooftopEvening
ShowCharacter native:/Characters/Alice/Normal
PlayBGM native:/Audio/QuietWind
ShowDialogue alice: You finally came.
PresentChoice
SetVariable affection.alice
JumpScene apologize_branch
PlaySFX native:/Audio/Choice
ShowDialogue alice: Then stay for a moment.
```
