# 核心边界、Cursive 升级与回归护栏设计

## 背景与目标

tbr++ 已完成 Rust 2024、gtk4-rs 0.11、CI 和 Windows 发布链现代化，但仍有三项相互关联的 P0 问题：应用只有一个二进制 crate，终端界面停留在 Cursive 0.20 并维护两份本地补丁，解析与排版的大版本升级缺少稳定回归样例。

本阶段在不改变阅读行为、SQLite schema、配置格式和发布产物名称的前提下完成以下目标：

1. 将应用实现移入 library target，通过一个小的启动接口供二进制入口调用；
2. 将 Cursive 升级到 0.21.1，删除 `vendor/cursive-core`、`vendor/safer-owning-ref` 和两个 Cargo patch；
3. 为 HTML/CSS、EPUB、横排、竖排和 SVG 建立确定性的语义回归测试；
4. 让默认 GUI、全功能测试和无默认功能终端构建继续通过。

本阶段不升级 Rusqlite、Stardict、Scraper、Lightning CSS、Resvg 或 Fontdb，也不改造 GTK 异步任务。这些工作使用本阶段建立的 seam 和测试护栏，在后续独立计划中实施。

## 方案比较

### 方案 A：渐进建立 library seam，再升级 Cursive（采用）

保留单 package 和单可执行文件。`src/main.rs` 只负责 CLI 解析，并调用 library 暴露的启动接口；现有模块与应用编排迁入 `src/lib.rs`。随后在保持 GUI/TUI 共用控制器的前提下，为终端 View 所持有的 trait object 建立 `Send + Sync` 契约，并升级 Cursive。

该方案改动可分批验证，不改变安装、运行和数据格式，也为后续无头测试和前端试点提供稳定 seam。

### 方案 B：立即拆成 Cargo workspace（暂不采用）

拆为 `tbr-core`、`tbr-gtk` 和 `tbr-tui` 能获得最强的编译期隔离，但会同时改变 package、二进制、资源嵌入、发布工作流和模块可见性。当前只有一个发布产物和两个前端适配器，workspace 的迁移成本高于本阶段收益。

### 方案 C：只升级 Cursive，不调整边界（不采用）

这能较快删除 vendor，但 Cursive 0.21 的 `View: Send + Sync` 会直接穿过 `ReadingView`、`Controller`、`Book`、`Container` 与 EPUB archive。若没有先明确核心 seam，只会把线程安全约束散落到现有二进制模块中，并继续保留无头测试和 feature 分层问题。

## 目标架构

### Library seam

新增 `src/lib.rs`，承接当前 `src/main.rs` 中的模块声明、资源嵌入、应用目录初始化，以及 GUI/TUI 切换流程。Library 对二进制只暴露：

```rust
pub struct LaunchOptions {
    pub terminal: bool,
    pub filename: Option<String>,
}

pub fn run(options: LaunchOptions) -> anyhow::Result<()>;
```

`src/main.rs` 保留 Clap 声明，将 CLI 值转换成 `LaunchOptions` 后调用 `tbr_plus::run`。内部 book、container、controller、parser 和 UI 模块不因 library target 而扩大为公共接口。

当前只被 GUI 使用的 `annotations` 和 `library` 模块继续受 `gui` feature 控制；测试通过 `cfg(test)` 编译其模块。这样无 GUI 构建不会携带暂时不可达的 GUI 功能，也不会用公开可见性掩盖 dead-code 警告。

### Cursive 线程安全 seam

Cursive 0.21 的 `View` 要求 `Send + Sync`。该约束通过终端 View 已经持有的核心对象向下传递，而不是通过 `unsafe impl` 绕过：

- `Book`、`Loader`、`Container`、`ContainerLoader` 与 EPUB archive trait 增加 `Send + Sync`；
- 相应 trait object 显式带有 `Send + Sync`，让编译器验证所有 adapter；
- `TerminalRender` 增加 `Send + Sync`；GTK 的 `GuiRender` 保持主线程本地，不增加线程安全约束；
- EPUB ZIP archive 的内部可变性从 `RefCell<ZipArchive<_>>` 改为互斥锁，并把锁中毒转换为带上下文的 `anyhow::Error`；
- 不添加 `unsafe impl Send/Sync`，不把 GTK 对象跨线程移动，也不引入 Tokio。

若某个只供 GUI 使用的缓存无法满足线程安全契约，应把缓存移到 GUI adapter，而不是弱化 Cursive 的约束或使用不安全标记。

### 回归测试 seam

测试只断言稳定的可观察行为，不锁定第三方库的内部类型或平台字体像素：

1. HTML/CSS fixture 验证容错 HTML、块级样式、标题、链接和中文文本产生的领域 `Line`/style 表示；
2. 最小 EPUB fixture 验证 container、OPF、导航和章节内容能通过 `BookLoader` 加载；
3. 横排与竖排 fixture 验证固定宽高下的字符顺序、换行和标点位置；
4. SVG fixture 验证嵌入图标可解析并产生非零尺寸 pixmap，不比较平台相关截图；
5. golden 文件采用 UTF-8 文本，规范化路径和换行，失败时输出可读 diff。

测试 fixture 放在 `tests/fixtures/`，测试代码优先位于对应模块的 `#[cfg(test)]` 中，以便访问未公开的领域表示。后续依赖升级必须先运行这些测试，再修改解析或字体实现。

## 数据流与错误处理

启动数据流保持不变：CLI → `LaunchOptions` → 配置/缓存目录 → 配置加载 → GTK 或终端前端。Library seam 不新增全局状态，不改变环境变量 `TBR_BOOK`，也不改变 GUI 关闭后回落到终端的现有行为。

EPUB archive 的锁获取失败必须返回包含目标文件或操作名称的错误；不得 panic。现有可恢复加载错误继续沿 `anyhow::Result` 返回到 GUI/终端。资源 fixture 读取失败在测试中可以使用 `expect`，但生产路径不得新增 `unwrap`。

## 实施边界

本阶段允许修改：

- `src/main.rs`、新增 `src/lib.rs`；
- core trait、trait object 与 EPUB archive 内部同步实现；
- `Cargo.toml`、`Cargo.lock`；
- 与 Cursive 0.21 API 兼容直接相关的终端文件；
- `tests/fixtures/` 和对应测试模块；
- CI 中针对 library/no-default feature 的检查。

本阶段明确不做：

- GTK 组件拆分或 Relm4 试点；
- SQLite schema 或数据库绑定升级；
- HTML/CSS、SVG、字体依赖升级；
- UI 视觉、快捷键、配置或阅读数据格式变化；
- Cargo workspace、多二进制或安装包命名调整。

## 验收标准

1. `cargo test --all-features` 通过，新增 fixture 测试确实在实现改动前出现预期失败；
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings` 通过；
3. `cargo build --no-default-features` 通过，且不再输出当前 78 条本项目 unused/dead-code 警告；
4. `cargo test --lib --no-default-features` 通过；
5. `cargo audit` 不再包含 `owning_ref`，Cargo 依赖图不再引用本地 `vendor` patch；
6. `vendor/cursive-core`、`vendor/safer-owning-ref` 和 `[patch.crates-io]` 全部删除；
7. 默认 GUI、`--terminal`、EPUB/TXT/HTML 加载、横排/竖排和输入法行为保持不变；
8. `git diff --check` 通过，未修改 SQLite、配置或导出格式。

## 后续阶段

本阶段完成后，按独立规格依次推进：

1. Stardict/Rusqlite 协调升级；
2. HTML/CSS 与 SVG/字体依赖分组升级；
3. GLib Future/async channel 替换 150 ms 轮询；
4. MSRV CI、cargo-deny 和发布矩阵；
5. 仅在核心 seam 稳定后评估 Relm4 单组件试点。
