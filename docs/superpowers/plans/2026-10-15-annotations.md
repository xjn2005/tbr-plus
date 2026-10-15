# tbr++ 0.1.0 本地批注与摘录导出 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 GTK 阅读器中提供本地持久高亮、批注、标签、回跳及稳定 Markdown 导出。

**Architecture:** `src/annotations/` 是不依赖 GTK 的纯 Rust 数据层，使用独立 SQLite 数据库。GUI 负责把当前书和选区交给数据层，并将当前章节的高亮范围传到既有横排/竖排共享渲染路径；临时选区继续拥有最高视觉优先级。

**Tech Stack:** Rust 2021、rusqlite、sha2、time、GTK4、Pango/Cairo、Fluent、GitHub Actions。

---

## 文件结构

- Create: `src/annotations/{mod,model,anchor,store,export}.rs` — 模型、锚点、持久化、导出。
- Create: `src/gui/annotations.rs` — 侧栏、选择浮层和导出对话框。
- Create: `tests/fixtures/annotations.txt` — 人工验证用的小型 UTF-8 文本。
- Modify: `Cargo.toml`, `src/main.rs`, `src/gui.rs`, `src/gui/view.rs`, `src/gui/render/{imp,xi,han}.rs`。
- Modify: `assets/i18n/{en_US,zh_CN}.ftl`, `assets/gui/gtk.css`, `README.md`。
- Create: `.github/workflows/ci.yml`。

### Task 1: 建立纯 Rust 模型与锚点

**Files:**
- Create: `src/annotations/model.rs`
- Create: `src/annotations/anchor.rs`
- Create: `src/annotations/mod.rs`
- Modify: `src/main.rs`, `Cargo.toml`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn reconnect_uses_context_to_choose_the_correct_duplicate_quote() {
    let lines = vec![Line::new("甲 shared 乙"), Line::new("丙 shared 丁")];
    let anchor = Anchor::new(0, 0, 0, 2, 8, &lines).unwrap();
    assert_eq!(anchor.reconnect(&lines), Some(ResolvedAnchor::single_line(0, 2..8)));
}

#[test]
fn anchor_uses_char_offsets_for_cjk_text() {
    let lines = vec![Line::new("你好，世界")];
    assert_eq!(Anchor::new(0, 0, 0, 1, 4, &lines).unwrap().quote, "好，世");
}
```

- [ ] **Step 2: 运行以确认失败**

Run: `cargo test annotations::anchor`

Expected: 编译失败，因为模块尚不存在。

- [ ] **Step 3: 实现最小模型和锚点**

```rust
pub struct Anchor {
    pub inner_book: usize, pub chapter: usize, pub line: usize,
    pub start_char: usize, pub end_char: usize,
    pub quote: String, pub prefix: String, pub suffix: String,
}
pub enum AnnotationColor { Yellow, Blue, Green, Red }
pub struct Annotation {
    pub id: i64, pub book_id: BookId, pub anchor: Anchor,
    pub color: AnnotationColor, pub note: Option<String>, pub tags: Vec<String>,
    pub created_at: OffsetDateTime, pub updated_at: OffsetDateTime,
}
```

`Anchor::new` 必须通过 `Line::char_at` 和 `Line::len` 处理字符索引，保存前后各 32 个字符。重连先匹配 `prefix + quote + suffix`，再匹配唯一 `quote`；歧义返回 `None`。

- [ ] **Step 4: 声明依赖与模块**

```toml
sha2 = "0.10"
time = { version = "0.3", features = ["formatting", "macros"] }
```

在 `main.rs` 增加 `mod annotations;`；`mod.rs` 只 re-export GUI 所需公开 API。

- [ ] **Step 5: 验证并提交**

Run: `cargo test annotations::`

Expected: PASS。

```bash
git add Cargo.toml Cargo.lock src/main.rs src/annotations
git commit -m "Add annotation model and anchors"
```

### Task 2: 加入独立数据库、书籍身份和 Markdown 导出

**Files:**
- Create: `src/annotations/store.rs`, `src/annotations/export.rs`
- Modify: `src/annotations/mod.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn reopening_a_database_preserves_tags_and_orders_annotations() {
    let path = std::env::temp_dir().join("tbr-plus-annotations-test.sqlite");
    let _ = std::fs::remove_file(&path);
    let mut store = AnnotationStore::open(&path).unwrap();
    let book = store.upsert_book(BookId::from_hex("00".repeat(32)).unwrap(), "Book", None, "book.txt").unwrap();
    store.create(book.id, draft("second", vec!["systems"])).unwrap();
    store.create(book.id, draft("first", vec!["rust", "systems"])).unwrap();
    drop(store);
    let store = AnnotationStore::open(&path).unwrap();
    let found = store.list_for_book(book.id).unwrap();
    assert_eq!(found.iter().map(|item| item.quote.as_str()).collect::<Vec<_>>(), ["second", "first"]);
    assert_eq!(found[1].tags, ["rust", "systems"]);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn fingerprint_is_independent_of_path() {
    assert_eq!(book_id(b"book", "Title", None), book_id(b"book", "Title", None));
}
#[test]
fn markdown_export_is_deterministic() {
    assert_eq!(export_book(&book, &annotations, false), expected_markdown);
}
```

- [ ] **Step 2: 运行以确认失败**

Run: `cargo test annotations::store annotations::export`

Expected: 编译失败，因为 `AnnotationStore` 和 `export_book` 尚未定义。

- [ ] **Step 3: 实现事务化存储**

创建 `info(version)`、`books`、`annotations`、`tags`、`annotation_tags` 及 `(book_id, inner_book, chapter, line)` 索引。所有 create/update/delete 标签操作在单个 SQLite transaction 内执行。书籍 ID 是“文件字节 + NUL + 规范化标题 + NUL + 可选作者”的流式 SHA-256；路径仅存 `last_path`，绝不参与 ID。

- [ ] **Step 4: 实现稳定导出**

```rust
pub fn export_book(book: &StoredBook, annotations: &[Annotation], with_front_matter: bool) -> String
```

按 `chapter, line, start_char, created_at, id` 排序，逐行 `> ` 引用，标签排序去重，Rfc3339 UTC 时间；空笔记和空标签不输出。

- [ ] **Step 5: 验证并提交**

Run: `cargo test annotations::`

Expected: PASS。

```bash
git add src/annotations Cargo.toml Cargo.lock
git commit -m "Store and export annotations locally"
```

### Task 3: 在共享渲染路径绘制持久高亮

**Files:**
- Modify: `src/gui/view.rs`, `src/gui/render/imp.rs`, `src/gui/render/xi.rs`, `src/gui/render/han.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn temporary_selection_wins_over_persistent_annotation() {
    let spans = [PersistentHighlight::new(4, 2..5, yellow())];
    assert_eq!(background_at(4, 3, None, &colors, &selection(), &spans),
               colors.highlight_background);
}
```

- [ ] **Step 2: 运行以确认失败**

Run: `cargo test gui::render::imp::tests::temporary_selection_wins_over_persistent_annotation`

Expected: 编译失败，因为 `PersistentHighlight` 尚未定义。

- [ ] **Step 3: 扩展 GuiView，不改变通用 Controller trait**

```rust
pub struct PersistentHighlight {
    pub line: usize, pub range: Range<usize>, pub background: Color32,
}
pub fn set_persistent_highlights(&self, highlights: Vec<PersistentHighlight>) {
    *self.imp().persistent_highlights.borrow_mut() = highlights;
}
```

`GuiView::redraw` 和 `full_redraw` 把 spans 传入 `GuiRender::gui_redraw` / `wrap_line`。共享 `update_for_highlight` 先保留现有 `HighlightInfo` 的颜色，再应用最后创建的持久 span；renderer 只接收范围和 `Color32`，不依赖数据库。

- [ ] **Step 4: 同步横排/竖排实现**

`GuiXiRender::wrap_line` 与 `GuiHanRender::wrap_line` 都将同一 slice 传给 `update_for_highlight`。

- [ ] **Step 5: 验证并提交**

Run: `cargo test --all-features gui::render::imp::tests && cargo build --no-default-features`

Expected: PASS。

```bash
git add src/gui/view.rs src/gui/render
git commit -m "Render persistent annotation highlights"
```

### Task 4: 构建批注侧栏和浮层

**Files:**
- Create: `src/gui/annotations.rs`
- Modify: `assets/gui/gtk.css`, `assets/i18n/en_US.ftl`, `assets/i18n/zh_CN.ftl`

- [ ] **Step 1: 实现可测试的组件边界**

```rust
pub(super) struct AnnotationPanel;
impl AnnotationPanel {
    pub fn new(i18n: &I18n) -> (Self, gtk4::Box);
    pub fn replace(&self, annotations: &[Annotation]);
    pub fn show_selection_actions(&self, parent: &GuiView, point: Rectangle,
                                  selection: SelectionDraft);
    pub fn set_callbacks(&self, callbacks: AnnotationCallbacks);
}
```

组件只通过闭包请求创建、编辑、删除、跳转和导出，不保存 SQLite 连接。

- [ ] **Step 2: 实现视觉层级**

使用 `Popover`，按钮固定为黄、蓝、绿、红、批注、复制、撤销；“批注”展开 `TextView` 与标签 `Entry`。侧栏使用 `ListBox` 卡片，主区域回跳，编辑/删除放进尾部菜单；无数据时居中显示提示。

- [ ] **Step 3: 加入主题自适应 CSS 和双语文案**

```css
.annotation-card { padding: 10px; border-radius: 8px; }
.annotation-quote { font-weight: 600; }
.annotation-tag { padding: 2px 6px; border-radius: 999px; }
.annotation-empty { opacity: 0.7; padding: 24px; }
```

颜色按钮使用 CSS class 和 opacity，不硬编码黑白；所有按钮配 i18n tooltip，且不抢占阅读焦点。

- [ ] **Step 4: 人工验证组件**

Run: `cargo run -- tests/fixtures/annotations.txt`

Expected: 明暗主题下浮层、空状态、卡片均不溢出，焦点返回阅读页。

- [ ] **Step 5: 提交**

```bash
git add src/gui/annotations.rs assets/gui/gtk.css assets/i18n
git commit -m "Add annotation panel interface"
```

### Task 5: 接通 GUI 生命周期、数据库和回跳

**Files:**
- Modify: `src/gui.rs`, `src/gui/annotations.rs`, `src/gui/view.rs`

- [ ] **Step 1: 增加 GUI 状态**

```rust
annotations: Rc<RefCell<AnnotationStore>>,
annotation_panel: AnnotationPanel,
current_annotation_book: RefCell<Option<StoredBook>>,
```

初始化路径为 `dirs::config_dir()/"tbr"/"tbr-plus.sqlite"`。无法创建数据库时调用 `GuiContext::error`，其余阅读功能继续可用。

- [ ] **Step 2: 实现刷新函数**

实现 `GuiContext::refresh_annotations()`：以容器文件和 `book_name(inner_book)` 得到/创建 `StoredBook`，查询当前 `inner_book + chapter`，更新 panel 与 `view.set_persistent_highlights`，最后只重绘一次。打开、重载、切换内书及章节跳转都调用它。

- [ ] **Step 3: 连接选区、右键菜单和撤销**

`TEXT_SELECTED_SIGNAL` 完成临时选区后显示浮层。颜色回调用当前 `controller.selected()` 和坐标创建 `Anchor`、写入 store、清理临时选区、刷新侧栏和渲染。复制复用 `copy_selection`；撤销删除刚创建的 ID；右键菜单注册相同高亮与批注 action。

- [ ] **Step 4: 实现编辑、删除、导出和回跳**

编辑更新 note/color/tags；删除成功才刷新。导出用 `FileDialog::save`，再用 `std::fs::write` 写导出器文本。回跳前 `Anchor::reconnect`，成功时调用 `controller.goto(inner_book, chapter, line, start, Some(range), context)`；失败显示“暂时无法回到原处”，保留数据。

- [ ] **Step 5: 加入侧栏与快捷键**

在 `setup_sidebar` 增加 `SIDEBAR_ANNOTATIONS_NAME`。添加 `Ctrl+Shift+A` 打开侧栏和 `Ctrl+Shift+H` 为当前选区创建黄色高亮；保留 `Ctrl+C`、词典及搜索快捷键。

- [ ] **Step 6: 手工验收并提交**

Run: `cargo run -- tests/fixtures/annotations.txt`

Expected: 横排和竖排均可创建四色高亮、添加说明/标签、重开后显示、卡片回跳、编辑/删除并导出；移动文件后重开同一内容仍能匹配。

```bash
git add src/gui.rs src/gui/annotations.rs src/gui/view.rs
git commit -m "Integrate local annotations into reader"
```

### Task 6: 加入 CI 和发布前验证

**Files:**
- Create: `.github/workflows/ci.yml`
- Modify: `README.md`
- Create: `tests/fixtures/annotations.txt`

- [ ] **Step 1: 添加人工验证文本**

创建 `tests/fixtures/annotations.txt`，内容如下：

```text
本地阅读值得回到原文。
Annotations must survive a renamed book.
中文选区不应按 UTF-8 字节截断。
```

- [ ] **Step 2: 添加 CI**

```yaml
name: CI
on: [push, pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo fmt --check
      - run: cargo clippy --all-targets --all-features -- -D warnings
      - run: cargo test --all-features
      - run: cargo build --no-default-features
```

- [ ] **Step 3: 更新 README**

在 About 后说明 GUI 能本地保存高亮、批注和标签、将当前书的摘录导出 Markdown，并明确没有云同步与 PDF 支持。

- [ ] **Step 4: 运行全套验证**

Run: `cargo fmt --check; cargo clippy --all-targets --all-features -- -D warnings; cargo test --all-features; cargo build --no-default-features`

Expected: 全部退出码为 0。

- [ ] **Step 5: 检查并提交**

```bash
git diff --check
git status --short
git add .github/workflows/ci.yml README.md
git commit -m "Add annotation quality checks"
```

## 计划自查

- Spec coverage：任务 1 覆盖锚点与 CJK；任务 2 覆盖独立 SQLite、稳定 ID、标签与导出；任务 3 覆盖横排/竖排持久高亮；任务 4-5 覆盖浮层、侧栏、编辑、删除、回跳和错误；任务 6 覆盖 CI 与质量门槛。
- Scope：没有加入终端批注、跨书搜索、Markdown/PDF 输入、同步或 AI 功能。
- Consistency：`Anchor`、`AnnotationStore`、`AnnotationPanel` 与 `PersistentHighlight` 的职责不重叠；renderer 不依赖数据层。
