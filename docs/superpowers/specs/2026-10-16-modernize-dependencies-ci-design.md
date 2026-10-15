# 依赖与 CI 现代化设计

## 背景与目标

tbr++ 是一个 Rust/GTK4 的本地优先阅读器。目前它采用 Rust 2021、`gtk4` 0.8
及同代 GTK Rust 绑定；CI 只验证格式、全功能测试和无默认功能构建。现有 20 个
单元测试通过，但构建仍有两个未使用字段警告。

本阶段的目标是在不改变阅读器行为和本地数据格式的前提下，建立可重复、可审计的
现代化依赖与质量门槛。GUI 架构、异步模型和交互体验的重构不包含在本阶段；它们将
在稳定基线之上单独设计和实施。

## 已选择的方案

采用平衡升级：升级语言版本和关键依赖，同时补全 CI 和依赖更新自动化。升级按小而
可验证的提交推进，不引入新的 GUI 框架或异步运行时。

相较于只补 CI，此方案会消除技术栈陈旧的问题；相较于整体重写，它不会将依赖风险
与架构、UI 变更耦合。

## 技术基线

- 新增 `rust-toolchain.toml`，固定 CI 与本地均使用 stable 工具链；包的
  `rust-version` 设为 `1.85`，这是 Rust 2024 的最低版本。
- 将 `edition` 从 `2021` 迁移为 `2024`，使用 `cargo fix --edition` 的结果作为
  机械迁移起点，再人工审查语义变化。
- 将 `gtk4` 从 0.8 升至 0.11 系列，并将 `glib`、`gio`、`pangocairo` 等 GTK Rust
  绑定同步到该系列所要求的兼容版本。`Cargo.lock` 是唯一锁定的精确版本来源，避免
  在 manifest 中人为混用不兼容的绑定世代。
- 其余依赖按 semver 兼容范围更新；保留 `rusqlite` 的 Windows `bundled` 特性和所有
  现有 feature 名称，避免改变安装要求或运行时行为。

## 模块和数据流

本阶段不改变应用模块间的调用或 SQLite schema。改动仅流经构建管线：

1. 本地开发与 CI 从 `rust-toolchain.toml` 读取同一工具链。
2. Cargo 按更新后的 manifest/lockfile 解析、构建 feature 组合。
3. GitHub Actions 依次运行格式、静态检查、测试、无 GUI 构建和依赖安全检查。
4. Dependabot 仅针对 Cargo 与 GitHub Actions 创建更新 PR；所有 PR 通过同一质量门。

## CI 质量门

工作流保持 Ubuntu 24.04，并以最小 `contents: read` 权限运行。使用 Cargo 缓存，
但不得跳过任何检查。质量门如下：

1. `cargo fmt --check`。
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`。
3. `cargo test --all-features`。
4. `cargo build --no-default-features`，确保终端模式不意外依赖 GUI。
5. `cargo audit`，用于检查锁文件中已公开披露的漏洞。

审计工具通过受维护的安装 action 安装，并锁定到主版本。若审计出现误报或没有修复
版本，例外必须放入版本控制的配置中，并附带原因、上游 issue 与到期条件；不允许在
工作流中静默忽略。

## 错误处理与兼容策略

- 升级暴露出的弃用 API 必须改为当前推荐接口；正常运行路径中新增的可恢复错误返回
  `Result` 并展示清晰消息，不能用 `unwrap` 掩盖失败。
- 仅修复本阶段触及的编译警告：`Annotation` 的 `book_id`、`updated_at` 与
  `SearchHit` 的 `tags`、`score` 要么在界面中实际消费，要么从模型/API 中移除并同步
  调整测试。不会借此做无关的大规模 `unwrap` 清理。
- 新旧构建仍支持默认 GUI 和 `--no-default-features` 的终端路径；SQLite 文件、批注锚点
  和导出 Markdown 格式保持兼容。

## 验证与验收标准

- 干净工作区可在稳定工具链上完成所有五个 CI 质量门。
- Clippy 以 `-D warnings` 通过，不留下当前死代码警告。
- `cargo test --all-features` 的现有 20 个测试全部保持通过；若 API 适配涉及行为变更，
  先补充回归测试再改实现。
- `cargo build --no-default-features` 能在 Linux CI 中通过。
- Dependabot 配置只触及 Cargo 与 GitHub Actions，采用每周分组更新以减少 PR 噪声。
- 升级完成后记录实际的工具链与主要依赖版本、已知兼容限制及本地 GTK 构建前置条件。

## 实施顺序

1. 添加工具链声明、Rust 2024 manifest 设置和 CI 的 lint 基线；清除现有警告。
2. 升级 GTK Rust 绑定和受其约束的依赖，逐一处理编译/API 适配。
3. 扩展 CI 安全门、Dependabot 与缓存；执行全套验证。
4. 将 GUI 状态拆分、GLib 异步任务和交互改进作为下一阶段独立规格。

## 不在范围内

- 不替换 GTK4、Cursive、SQLite 或书籍解析器。
- 不变更批注、书库或配置的数据格式。
- 不引入账号、同步、网络服务、遥测或 AI 功能。
- 不进行 GUI 大模块拆分、运行时替换或视觉重设计。
