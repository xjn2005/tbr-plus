# Dependency and CI Modernization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move tbr++ to the Rust 2024/modern GTK Rust baseline and make every pull request pass repeatable lint, test, build, and vulnerability checks.

**Architecture:** Keep the application architecture and SQLite schema unchanged. `Cargo.toml`, `Cargo.lock`, and `rust-toolchain.toml` define a consistent local/CI toolchain; the quality workflow consumes that toolchain and enforces the same feature combinations developers use. Remove only model fields that are provably unused so the stronger lint gate has no exemptions.

**Tech Stack:** Rust 2024 (MSRV 1.92), Cargo, gtk4-rs 0.11 family, GitHub Actions, Dependabot, cargo-audit.

---

## File structure

- Create: `rust-toolchain.toml` — declares the shared Rust channel and required developer components.
- Create: `.github/dependabot.yml` — schedules grouped Cargo and GitHub Actions updates.
- Modify: `Cargo.toml` — Rust edition/MSRV and GTK Rust dependency family.
- Modify: `Cargo.lock` — Cargo-generated resolution for the upgraded manifest.
- Modify: `.github/workflows/ci.yml` — cached, least-privilege quality and audit gates.
- Modify: `src/annotations/model.rs` — removes unused transport fields from `Annotation`.
- Modify: `src/annotations/store.rs` — updates annotation row mapping and helper signature after field removal.
- Modify: `src/annotations/export.rs` — updates test fixtures to match `Annotation`.
- Modify: `src/library/index.rs` — removes unused `SearchHit` fields and unneeded SQL projections.
- Modify: `README.md` — records the stable Rust/GTK build prerequisites and quality commands.

### Task 1: Establish the Rust 2024 baseline

**Files:**
- Create: `rust-toolchain.toml`
- Modify: `Cargo.toml:1-10`
- Modify: `Cargo.lock`

- [ ] **Step 1: Record the initial compiler baseline**

Run: `cargo check --all-features`

Expected: exit status 0 and warnings only for `Annotation::{book_id, updated_at}` and `SearchHit::{tags, score}`. Save no generated output in the repository.

- [ ] **Step 2: Declare the shared stable toolchain**

Create `rust-toolchain.toml` with:

```toml
[toolchain]
channel = "stable"
profile = "minimal"
components = ["clippy", "rustfmt"]
```

Update the package header in `Cargo.toml` to:

```toml
categories = ["command-line-utilities"]
rust-version = "1.92"
edition = "2021"
```

- [ ] **Step 3: Apply the edition migration and format the result**

Run: `cargo fix --edition --allow-dirty --allow-staged`

Then run: `cargo fmt --all`

Expected: Cargo changes only source constructs required for Rust 2024 compatibility; formatting exits 0.

- [ ] **Step 4: Mark the completed edition migration in the manifest**

Change the package header to:

```toml
categories = ["command-line-utilities"]
rust-version = "1.92"
edition = "2024"
```

- [ ] **Step 5: Regenerate the lockfile without manually editing it**

Run: `cargo update`

Expected: `Cargo.lock` changes only through Cargo, and `cargo metadata --format-version 1 --no-deps` exits 0.

- [ ] **Step 6: Verify the baseline before changing GTK bindings**

Run: `cargo check --all-features`

Expected: exit status 0; the only remaining warnings are the four known unused model fields.

- [ ] **Step 7: Commit the toolchain baseline**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml src
git commit -m "Upgrade project to Rust 2024"
```

Expected: one commit containing the edition migration and no unrelated files.

### Task 2: Remove the current unused model fields without changing persisted data

**Files:**
- Modify: `src/annotations/model.rs:151-163`
- Modify: `src/annotations/store.rs:213-280`
- Modify: `src/annotations/export.rs:81-103`
- Modify: `src/library/index.rs:23-35,172-210`

- [ ] **Step 1: Remove unused annotation APIs, then update the model and its test fixtures**

Delete `BookId::from_content` from `src/annotations/model.rs`; its only behavior is duplicated by the tested `BookId::from_file` path and it has no callers. Delete `AnnotationStore::list_for_chapter` from `src/annotations/store.rs`; it has no callers and all persisted-data access remains through `list_for_book`.

Change `Annotation` to retain only fields consumed outside storage:

```rust
pub struct Annotation {
    pub id: i64,
    pub anchor: Anchor,
    pub color: AnnotationColor,
    pub note: Option<String>,
    pub tags: Vec<String>,
    pub starred: bool,
    pub status: AnnotationStatus,
    pub created_at: i64,
}
```

Delete `book_id` and `updated_at` from both literal fixtures in `src/annotations/export.rs`. Keep the database schema and `created_at`/`updated_at` columns unchanged.

- [ ] **Step 2: Make the annotation store mapping match the smaller model**

Change the private helper signature and call sites from:

```rust
fn list<P: Params>(&self, query: &str, params: P, book_id: &BookId) -> Result<Vec<Annotation>>
```

to:

```rust
fn list<P: Params>(&self, query: &str, params: P) -> Result<Vec<Annotation>>
```

Remove `book_id: book_id.clone()` and `updated_at: row.get(15)?` from the `Annotation` mapping. Keep the `updated_at` SQL projection temporarily so the query schema stays stable during this narrow model cleanup; `rusqlite` safely permits unread result columns.

- [ ] **Step 3: Remove unused search response fields and projections**

Change `SearchHit` to:

```rust
pub struct SearchHit {
    pub book_id: BookId,
    pub book_title: String,
    pub source_path: Option<String>,
    pub chapter: String,
    pub inner_book: usize,
    pub chapter_index: usize,
    pub line: usize,
    pub body: String,
    pub note: Option<String>,
}
```

In `LibraryIndex::search`, remove only the `tags` and `score` assignments from the `SearchHit` mapping. Keep the existing SQL select list and `order by 8` unchanged: removing dead Rust transport fields must not alter result ordering or database behavior in this phase.

- [ ] **Step 4: Run focused regressions**

Run: `cargo test annotations::export::tests::markdown_export_is_stable_and_orders_annotations_by_location library::index::tests::index_searches_cjk_body_and_notes --all-features`

Expected: both tests pass. Then run `cargo clippy --all-targets --all-features -- -D warnings`; expected exit status 0 with no warning suppression attributes added.

- [ ] **Step 5: Commit the warning cleanup**

```bash
git add src/annotations/model.rs src/annotations/store.rs src/annotations/export.rs src/library/index.rs
git commit -m "Remove unused annotation and search fields"
```

Expected: one commit with no SQLite schema migration and no changed public behavior.

### Task 3: Upgrade the GTK Rust binding family

**Files:**
- Modify: `Cargo.toml:40-82`
- Modify: `Cargo.lock`
- Modify: `src/gui.rs`
- Modify: `src/gui/view.rs`
- Modify: `src/gui/settings.rs`
- Modify: other `src/gui/**/*.rs` files reported by `cargo check --all-features`

- [ ] **Step 1: Set matching GTK Rust dependency requirements**

Update the GUI dependency declarations to:

```toml
gtk4 = { version = "0.11", features = ["v4_12"], optional = true }
pangocairo = { version = "0.22", optional = true }
```

Do not add direct `glib`, `gio`, `pango`, `gdk4`, or `cairo-rs` dependencies: the compatible family must be selected transitively by `gtk4` and `pangocairo`.

- [ ] **Step 2: Resolve and inspect the dependency graph**

Run: `cargo update -p gtk4 -p pangocairo`

Run: `cargo tree -i gtk4`

Expected: one GTK Rust binding family is resolved; no direct dependency is forcing the old 0.19 GLib family.

- [ ] **Step 3: Apply compiler-directed GTK API adaptations**

Run: `cargo check --all-features`

For every compiler error in `src/gui.rs`, `src/gui/view.rs`, `src/gui/settings.rs`, or their submodules, replace only the removed/renamed GTK binding API with its compiler-suggested 0.11 equivalent. Preserve existing callback capture, widget ownership, CSS loading, and error-display logic. Do not add a new async runtime or alter widget layout in this task.

- [ ] **Step 4: Verify the GUI and terminal feature combinations**

Run: `cargo check --all-features`

Expected: exit status 0.

Run: `cargo build --no-default-features`

Expected: exit status 0; no GTK system package is required for this build.

- [ ] **Step 5: Commit the binding upgrade**

```bash
git add Cargo.toml Cargo.lock src/gui.rs src/gui
git commit -m "Upgrade GTK Rust bindings"
```

Expected: one commit containing only generated lockfile changes and API compatibility edits required by the 0.11 upgrade.

### Task 4: Add a complete, cached CI quality gate and dependency update automation

**Files:**
- Create: `.github/dependabot.yml`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add weekly grouped Dependabot updates**

Create `.github/dependabot.yml` with:

```yaml
version: 2
updates:
  - package-ecosystem: cargo
    directory: "/"
    schedule:
      interval: weekly
    groups:
      rust-dependencies:
        patterns: ["*"]
  - package-ecosystem: github-actions
    directory: "/"
    schedule:
      interval: weekly
    groups:
      github-actions:
        patterns: ["*"]
```

- [ ] **Step 2: Replace the quality workflow with least-privilege cached checks**

Replace `.github/workflows/ci.yml` with:

```yaml
name: CI

on:
  push:
  pull_request:

permissions:
  contents: read

jobs:
  quality:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: Install GTK development files
        run: sudo apt-get update && sudo apt-get install -y libgtk-4-dev libpango1.0-dev pkg-config
      - run: cargo fmt --check
      - run: cargo clippy --workspace --all-targets --all-features -- -D warnings
      - run: cargo test --all-features
      - run: cargo build --no-default-features
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-audit
      - run: cargo audit
```

- [ ] **Step 3: Validate the workflow syntax and all locally available gates**

Run: `cargo fmt --check`

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Run: `cargo test --all-features`

Run: `cargo build --no-default-features`

Expected: every command exits 0. Review the YAML manually to confirm the workflow has only `contents: read` permissions and that `cargo-audit` is installed before it runs.

- [ ] **Step 4: Commit the CI and update automation**

```bash
git add .github/workflows/ci.yml .github/dependabot.yml
git commit -m "Harden Rust quality checks"
```

Expected: one commit enabling CI lint/audit checks and Dependabot without changing release workflow behavior.

### Task 5: Document and run the final acceptance suite

**Files:**
- Modify: `README.md:快速开始`
- Modify: `Cargo.lock`

- [ ] **Step 1: Document the required local toolchain and verification command**

After the existing sentence “安装 Rust 与 GTK 4 开发环境后：”, insert:

````markdown
仓库会通过 `rust-toolchain.toml` 自动选择稳定 Rust 工具链；Linux 还需要 GTK 4、Pango 和 `pkg-config` 开发包。提交前可运行：

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --no-default-features
```
````

- [ ] **Step 2: Run the final quality suite**

Run these commands in order:

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --no-default-features
cargo audit
git diff --check
git status --short
```

Expected: every quality command exits 0, `git diff --check` has no output, and `git status --short` lists only the README before the final commit.

- [ ] **Step 3: Commit documentation and report resolved versions**

```bash
git add README.md Cargo.lock
git commit -m "Document modern Rust development workflow"
git log --oneline -5
```

Expected: the log shows the five focused modernization commits, and the implementation handoff reports the final Rust toolchain, gtk4, pangocairo, and audit results.
