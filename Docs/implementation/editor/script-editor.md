# AstraEditor Script Editor 设计

Script Editor 是 `.astra` 文本的权威编辑面板。它基于 QML `TextArea` + Rust 后端（tree-sitter + ropey），Stage 4 实现行号 gutter、错误/警告 inline marker、source map badge 和查找/替换，为 Stage 5 的 astra-lsp Language Server 铺路。

## 1. 架构概览

```
用户输入
  ↓
QML TextArea (输入层)
  ↓ 文本变更 → Rust Bridge (script_editor.rs)
  ↓
ropey::Rope (权威文本缓冲区)
  ↓ 增量更新
tree-sitter (`.astra` grammar 解析)
  ↓
TokenList (token 类型 + 范围)
  ↓ signal: highlightTokensChanged
QML SyntaxHighlighter (渲染颜色)
  ↓

CompileReport (来自 astra-vn-script)
  ↓ signal: diagnosticsChanged
QML Gutter + InlineMarker (错误/警告波浪线)
  ↓

SourceMapRef (来自 astra-vn-editor)
  ↓ signal: sourceMappingChanged
QML SourceMapBadge (点击跳转 Graph 节点)
```

---

## 2. Rust 文本引擎

### 2.1 ropey 文本缓冲区

`ropey` 提供高效的 Rope 数据结构，支持大文件和频繁增量编辑。Bridge 侧 `script_editor.rs` 持有 `Rope`：

```rust
// Editor/Source/Bridge/astra-editor-bridge/src/script_editor.rs

use ropey::Rope;
use tree_sitter::{Parser, Tree, InputEdit};

pub struct ScriptEditorState {
    rope:        Rope,
    ts_tree:     Option<Tree>,
    ts_parser:   Parser,           // tree-sitter `.astra` grammar
    diagnostics: Vec<Diagnostic>,
    source_map:  Option<SourceMapRef>,
}

impl ScriptEditorState {
    /// QML 调用：文本发生 incremental 变化
    pub fn apply_edit(&mut self, edit: TextEdit) -> TokenUpdateResult {
        // 1. 更新 ropey rope
        self.rope.remove(edit.byte_range.clone());
        self.rope.insert(edit.byte_range.start, &edit.new_text);

        // 2. 构造 tree-sitter InputEdit
        let ts_edit = InputEdit { ... };

        // 3. 增量重新解析
        if let Some(old_tree) = &self.ts_tree {
            let new_tree = self.ts_parser.parse_with(
                &mut |byte_offset, _| {
                    // 从 rope 提供 UTF-8 chunks
                    self.rope.chunk_at_byte(byte_offset)
                },
                Some(old_tree),
            );
            self.ts_tree = new_tree;
        }

        // 4. 返回更新的 token 列表（仅受影响行）
        self.extract_tokens_for_changed_lines(&edit)
    }

    /// 接收 Bridge compile 结果，更新 diagnostics
    pub fn update_diagnostics(&mut self, report: &CompileReport) {
        self.diagnostics = report.diagnostics.iter()
            .map(|d| Diagnostic { range: d.source_span.into(), severity: d.severity, message: d.message.clone() })
            .collect();
    }

    /// 接收 source map，更新 badge 位置
    pub fn update_source_map(&mut self, source_map: SourceMapRef) {
        self.source_map = Some(source_map);
    }
}
```

### 2.2 tree-sitter `.astra` Grammar

为 `.astra` 语法编写 tree-sitter grammar（`tree-sitter-astra`，位于 `Engine/Source/Developer/astra-tool/tree-sitter-astra/`）：

```javascript
// grammar.js（tree-sitter grammar 定义）
module.exports = grammar({
  name: 'astra',
  rules: {
    source_file: $ => repeat($._statement),
    _statement: $ => choice(
      $.label_decl,     // :: label_name
      $.command,        // command_name args...
      $.choice_block,   // choice: ...
      $.if_block,       // if condition: ...
      $.comment,        // # comment
      $.stable_id,      // #@id some_id
    ),
    command: $ => seq(
      field('name', $.identifier),
      optional(field('character', seq('@', $.identifier))),
      ':',
      field('text', $.string_or_block),
    ),
    // ...
  }
});
```

Token 类型对应高亮分类：

| tree-sitter node type | `AstraTokenKind` | QML 颜色 |
| --- | --- | --- |
| `identifier`（命令名） | `CommandName` | `#79b3e8`（accent 蓝） |
| `keyword`（`choice`, `if`, `jump`, `wait`） | `Keyword` | `#c678dd`（紫） |
| `string` | `String` | `#98c379`（绿） |
| `comment` | `Comment` | `#5c6370`（灰） |
| `stable_id`（`#@id`） | `StableId` | `#e5c07b`（金） |
| `label_decl`（`::`） | `Label` | `#61afef`（浅蓝） |
| `ERROR` | `Error` | `#e05c5c`（红）|

---

## 3. QML Script Editor 组件

### 3.1 整体布局

```qml
// panels/ScriptEditor.qml
Item {
    id: scriptEditorRoot

    // 顶部工具栏
    ScriptEditorToolBar {
        onCompileClicked: bridge.compileStory()
        onFormatClicked:  bridge.formatAstra()   // Stage 5
        compilingStatus:  bridge.compileStatus   // "idle" | "compiling" | "error" | "ok"
    }

    // 主编辑区
    Row {
        // 行号 Gutter
        Gutter {
            id: gutter
            width: 48
            lineCount:        bridge.scriptLineCount
            errorLines:       bridge.diagnosticLineNumbers    // Set<int>
            warningLines:     bridge.warningLineNumbers
            sourceMappedLines: bridge.sourceMappedLineNumbers  // 显示 source map badge 图标
            onSourceBadgeClicked: function(line) {
                bridge.revealGraphNodeAtLine(line)
            }
        }

        // 文本编辑区
        ScrollView {
            TextArea {
                id: textArea
                font.family:    tokens.fontMono
                font.pixelSize: tokens.fontSizeNormal
                color:          tokens.textPrimary
                background:     Rectangle { color: tokens.bg1 }
                wrapMode:       TextEdit.NoWrap

                // 文本变化时通知 Bridge（debounced 150ms）
                onTextChanged: Qt.callLater(function() {
                    bridge.onScriptTextChanged(textArea.text,
                                               textArea.cursorPosition)
                })
            }

            // 语法高亮 overlay（QQuickPaintedItem 绘制彩色范围）
            SyntaxHighlightOverlay {
                anchors.fill: parent
                tokens: bridge.highlightTokens   // [{start, end, kind}]
            }

            // 错误/警告波浪线 overlay
            DiagnosticUnderlineOverlay {
                anchors.fill: parent
                diagnostics: bridge.diagnostics  // [{start, end, severity}]
            }
        }
    }

    // 底部诊断摘要栏
    DiagnosticSummaryBar {
        errorCount:   bridge.errorCount
        warningCount: bridge.warningCount
        onItemClicked: function(diagnostic) {
            textArea.cursorPosition = diagnostic.startByte
        }
    }

    // 查找/替换面板（Ctrl+F 触发显示）
    FindReplacePanel {
        id: findReplace
        visible: false
        onFindNext: function(query, caseSensitive, regex) {
            bridge.findNext(query, caseSensitive, regex)
        }
        onReplace: function(query, replacement) {
            bridge.replaceAt(bridge.currentFindMatch, replacement)
        }
        onReplaceAll: function(query, replacement) {
            bridge.replaceAll(query, replacement)
        }
    }
}
```

### 3.2 行号 Gutter

Gutter 用 `QQuickPaintedItem`（或纯 QML `Canvas`）绘制行号：

- 行号显示为右对齐数字，`textSecondary` 颜色。
- 有错误的行：行号背景变红色（`#3d1c1c`），行号字体变 `textError`。
- 有警告的行：行号背景变黄色（`#2d2a1a`）。
- 有 source map badge 的行：行号右侧显示小菱形图标（`◆`，accent 蓝）；点击时 Bridge 调用 `revealGraphNodeAtLine()` 跳转 Graph Editor。

### 3.3 错误/警告 Inline Marker

`DiagnosticUnderlineOverlay` 绘制波浪线：

- 错误：`#e05c5c` 红色波浪线（类 VS Code）
- 警告：`#e8b86d` 黄色波浪线
- hover 显示 tooltip（diagnostic message）

实现方式：`QQuickPaintedItem.paint()` 中用 `QPainter::drawPath()` 绘制正弦曲线（每像素 6px 宽度），范围从 Bridge 侧 `diagnostics` signal 更新。

---

## 4. Undo/Redo 分层实现

### 4.1 Script Editor 局部 Undo

ropey 不内置 undo 历史，需要在 `ScriptEditorState` 中维护：

```rust
pub struct ScriptEditorState {
    rope:       Rope,
    undo_stack: Vec<TextEdit>,   // 可撤销的编辑历史（局部栈）
    redo_stack: Vec<TextEdit>,
    // ...
}

impl ScriptEditorState {
    pub fn undo(&mut self) -> Option<TextEdit> {
        let edit = self.undo_stack.pop()?;
        // 逆向应用 edit 到 rope
        let reverse_edit = edit.reverse(&self.rope);
        self.rope.apply(&reverse_edit);
        self.redo_stack.push(edit);
        Some(reverse_edit)
    }
}
```

Ctrl+Z 在 Script Editor 内触发局部 undo，不影响全局 patch 历史。

### 4.2 提交到全局 Patch 历史

当 compile 被触发时（用户点击 Compile、切换到 Graph Editor 等），Bridge 将当前 rope 文本与上次 compile 基线对比，生成 `SourcePatch`，写入全局 patch 历史栈。

```rust
pub fn commit_to_global_history(&mut self, global_undo: &mut GlobalPatchHistory) {
    let baseline = self.last_compiled_text.clone();
    let current  = self.rope.to_string();
    if baseline != current {
        let patch = SourcePatch::diff(&baseline, &current);
        global_undo.push(patch);
        self.undo_stack.clear();  // 局部栈清空，全局接管
        self.last_compiled_text = current;
    }
}
```

---

## 5. 查找/替换

`FindReplacePanel` 的 Bridge 侧实现（`script_editor.rs`）：

```rust
pub fn find_next(&mut self, query: &str, case_sensitive: bool, regex: bool)
    -> Option<(usize, usize)>  // (start_byte, end_byte)
{
    // 在 ropey rope 上搜索（chunk 迭代，不全量 to_string）
    // 支持普通字符串和正则（regex crate）
    ...
}

pub fn replace_at(&mut self, range: (usize, usize), replacement: &str) {
    self.apply_edit(TextEdit { byte_range: range.0..range.1, new_text: replacement.to_string() });
    // 写入局部 undo 栈
}

pub fn replace_all(&mut self, query: &str, replacement: &str) -> usize {
    // 收集所有匹配位置（从后往前替换，避免偏移失效）
    ...
}
```

---

## 6. Source Map Badge → Graph 跳转

当 compile 成功后，`astra-vn-editor` 返回 `SourceMapRef`（行号 → command_id 映射）。Bridge 侧：

```rust
pub fn update_source_map(&mut self, source_map: SourceMapRef) {
    self.source_map = Some(source_map);
    // 发 signal：sourceMappedLineNumbers 更新
    // QML 更新 Gutter 中的 badge 显示
}

pub fn reveal_graph_node_at_line(&self, line: usize) -> Option<StableId> {
    let command_id = self.source_map.as_ref()?.line_to_command(line)?;
    // Graph Editor 侧收到 revealNodeById(command_id) signal
    Some(command_id)
}
```

Graph Editor 收到 `revealNodeById` 信号后，NodeEditor-Qt 将对应节点滚动到视口中央并选中（高亮边框）。

---

## 7. Stage 5 规划：astra-lsp

`astra-lsp` 是 `.astra` 语言的 Language Server（LSP 协议），为 Stage 5 规划，不影响 Stage 4 实现。

计划实现：

| LSP 功能 | 依赖 |
| --- | --- |
| 悬停提示（命令签名、参数说明） | `astra-vn-commands` schema |
| 跳转到定义（source map → command_id） | `astra-vn-editor` SourceMapRef |
| 错误内联（parse + type check） | `astra-vn-script` compile report |
| 代码补全（命令名、角色名、标签名） | `astra-vn-script` manifest |
| 代码折叠（`if`/`choice` 块） | tree-sitter grammar |

外部编辑器接入（VS Code、Neovim、Helix）通过标准 LSP stdio transport；AstraEditor 内置接入通过 in-process LSP client（替换现有 tree-sitter token 路径）。

---

## 8. 验收标准

```bash
# Script Editor 测试（S4-EDITOR-01 部分）
cargo test -p astra-editor-bridge script_editor_highlight
cargo test -p astra-editor-bridge script_editor_diagnostics
cargo test -p astra-editor-bridge script_editor_find_replace
cargo test -p astra-editor-bridge script_editor_source_map_badge
```

| 测试 | 描述 |
| --- | --- |
| `highlight_tokens` | 加载 `.astra` 文件 → token 列表覆盖所有命令名/关键字/字符串 |
| `error_marker` | 注入 parse error → diagnostic 出现在对应行 |
| `warning_marker` | 注入未知命令警告 → 黄色波浪线出现 |
| `source_map_badge` | compile 成功 → source map badge 出现在正确行 → 点击跳转 Graph |
| `find_next` | 查找字符串 → 光标跳转到第一个匹配 |
| `replace_all` | 全量替换 → 所有匹配项被替换 → undo 一步恢复 |
| `undo_local` | 局部编辑 → Ctrl+Z → 文字回退（不触发全局历史） |
| `undo_global_after_compile` | 编辑 → compile → 全局 patch 写入 → Ctrl+Shift+Z → source 回退 |
