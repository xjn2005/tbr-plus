# Core, Cursive, and Regression Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the application behind a small library seam, upgrade to Cursive 0.21.1 without unsafe thread markers, delete the vendored patches, and add deterministic parser/EPUB/TUI/SVG regression coverage.

**Architecture:** Keep one Cargo package and one executable. `src/main.rs` parses CLI arguments and calls `tbr_plus::run`; private library modules retain existing behavior. Cursive's `View: Send + Sync` requirement is satisfied at the existing `Book`, `Container`, EPUB archive, and terminal-render seams, while GTK renderer types remain main-thread local.

**Tech Stack:** Rust 2024, Cargo, Cursive 0.21.1, Crossterm, GTK4, standard-library `Mutex`, existing Rust test framework.

---

## File structure

- Create: `src/lib.rs` — owns embedded assets, private modules, launch options, and application startup.
- Modify: `src/main.rs` — keeps only Clap parsing and delegates to the library.
- Create: `tests/library_api.rs` — proves the library launch interface exists.
- Modify: `src/book.rs` — adds thread-safety contracts to books and loaders.
- Modify: `src/container.rs` — adds thread-safety contracts to containers and loaders.
- Modify: `src/book/epub.rs` — replaces archive `RefCell` with `Mutex` and propagates lock errors.
- Modify: `src/terminal/view.rs` — adds the terminal renderer contract and compile-time regression test.
- Modify: `Cargo.toml`, `Cargo.lock` — upgrades Cursive and removes local patches.
- Delete: `vendor/cursive-core/`, `vendor/safer-owning-ref/` — removes obsolete forks.
- Create: `tests/fixtures/reader_sample.html` — HTML/CSS characterization input.
- Create: `tests/fixtures/minimal_epub/META-INF/container.xml` — extracted EPUB root descriptor.
- Create: `tests/fixtures/minimal_epub/OEBPS/content.opf` — minimal EPUB package.
- Create: `tests/fixtures/minimal_epub/OEBPS/toc.ncx` — EPUB 2 navigation.
- Create: `tests/fixtures/minimal_epub/OEBPS/chapter.xhtml` — CJK chapter fixture.
- Modify: `src/html_parser.rs`, `src/book/epub.rs`, `src/terminal/view/xi.rs`, `src/terminal/view/han.rs`, `src/gui.rs` — adds semantic regressions using existing interfaces and assets.
- Modify: `.github/workflows/ci.yml` — verifies library/no-default builds and treats warnings as failures.

### Task 1: Establish the library seam

**Files:**
- Create: `tests/library_api.rs`
- Create: `src/lib.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the failing public-interface test**

Create `tests/library_api.rs`:

```rust
#[test]
fn launch_options_are_public() {
    let options = tbr_plus::LaunchOptions {
        terminal: true,
        filename: Some("book.txt".to_owned()),
    };

    assert!(options.terminal);
    assert_eq!(options.filename.as_deref(), Some("book.txt"));
}
```

- [ ] **Step 2: Run the test and verify RED**

Run: `cargo test --test library_api --no-default-features`

Expected: compilation fails because there is no library crate named `tbr_plus`.

- [ ] **Step 3: Move application ownership into `src/lib.rs`**

Create `src/lib.rs` with the current private module graph and this complete launch adapter:

```rust
#[macro_use]
extern crate markup5ever;

use anyhow::{Result, anyhow};
use dirs::{cache_dir, config_dir};
use rust_embed::RustEmbed;

use crate::book::BookLoader;
use crate::common::Position;
use crate::config::load_config;
use crate::container::ContainerManager;
#[cfg(feature = "i18n")]
use crate::i18n::I18n;

mod annotations;
mod book;
mod color;
mod common;
mod config;
mod container;
mod controller;
#[cfg(feature = "gui")]
mod gui;
mod html_parser;
#[cfg(feature = "i18n")]
mod i18n;
mod library;
mod list;
#[cfg(feature = "open")]
mod open;
mod terminal;
mod xhtml;

const TBR_BOOK_ENV_KEY: &str = "TBR_BOOK";

#[macro_export]
macro_rules! description {
    () => {
        "Local-first ebook reader,"
    };
}

#[macro_export]
macro_rules! version {
    () => {
        env!("CARGO_PKG_VERSION")
    };
}

#[macro_export]
macro_rules! version_string {
    () => {
        concat!(description!(), " v", version!())
    };
}

#[macro_export]
macro_rules! package_name {
    () => {
        env!("CARGO_PKG_NAME")
    };
}

#[derive(RustEmbed)]
#[folder = "assets/"]
#[prefix = ""]
#[include = "*.toml"]
#[include = "*.svg"]
#[include = "*.ttc"]
#[include = "*.ftl"]
#[include = "*.png"]
struct Asset;

pub struct LaunchOptions {
    pub terminal: bool,
    pub filename: Option<String>,
}

pub fn run(options: LaunchOptions) -> anyhow::Result<()> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Can not find config dir."))?
        .join(package_name!());
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| anyhow::anyhow!("Can not find cache dir."))?
        .join(package_name!());
    let config_file = config_dir.join("tbr.toml");
    let filename = options
        .filename
        .map_or_else(|| std::env::var(TBR_BOOK_ENV_KEY).ok(), Some);
    #[allow(unused_mut)]
    let (mut current, mut configuration) =
        load_config(filename, config_file, &config_dir, &cache_dir)?;
    #[cfg(feature = "gui")]
    if !options.terminal {
        match gui::start(current, configuration)? {
            Some((curr, config)) => {
                current = curr;
                configuration = config;
            }
            None => return Ok(()),
        }
    }
    terminal::start(current, configuration, config_dir)
}
```

Keep `Asset`, `TBR_BOOK_ENV_KEY`, and all module declarations private.

- [ ] **Step 4: Reduce `src/main.rs` to the binary adapter**

Replace `src/main.rs` with:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use clap::Parser;
use tbr_plus::{LaunchOptions, run};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[cfg(feature = "gui")]
    #[clap(
        short,
        long,
        help = "Using terminal to read e-book, by default if gui exists, tbr will using gui view."
    )]
    terminal: bool,
    filename: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    run(LaunchOptions {
        #[cfg(feature = "gui")]
        terminal: cli.terminal,
        #[cfg(not(feature = "gui"))]
        terminal: true,
        filename: cli.filename,
    })
}
```

- [ ] **Step 5: Verify GREEN and existing behavior**

Run: `cargo test --test library_api --no-default-features`

Expected: 1 test passes.

Run: `cargo test --all-features`

Expected: all existing tests and `library_api` pass.

- [ ] **Step 6: Commit the library seam**

```powershell
git add src/lib.rs src/main.rs tests/library_api.rs
git commit -m "Extract application library seam"
```

### Task 2: Make the no-GUI library boundary warning-clean

**Files:**
- Modify: `src/lib.rs`

- [ ] **Step 1: Prove the current no-default boundary is RED**

Run: `cargo check --lib --no-default-features -- -D warnings`

Expected: failure from unused GUI-only annotation/library imports or dead code.

- [ ] **Step 2: Gate only currently GUI-owned capabilities**

Change the two module declarations in `src/lib.rs` to:

```rust
#[cfg(any(feature = "gui", test))]
mod annotations;
#[cfg(any(feature = "gui", test))]
mod library;
```

Do not make internal modules public and do not introduce new feature names.

- [ ] **Step 3: Verify the feature boundary is GREEN**

Run: `cargo check --lib --no-default-features -- -D warnings`

Expected: exit 0 with no project warnings.

Run: `cargo test --lib --no-default-features`

Expected: library unit tests pass without GTK.

- [ ] **Step 4: Commit the feature cleanup**

```powershell
git add src/lib.rs
git commit -m "Gate GUI-only library capabilities"
```

### Task 3: Make terminal state safely `Send + Sync`

**Files:**
- Modify: `src/terminal/view.rs`
- Modify: `src/book.rs`
- Modify: `src/container.rs`
- Modify: `src/book/epub.rs`

- [ ] **Step 1: Add a compile-time failing regression**

Append to `src/terminal/view.rs`:

```rust
#[cfg(test)]
mod send_sync_tests {
    use super::ReadingView;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn reading_view_is_send_sync() {
        assert_send_sync::<ReadingView>();
    }
}
```

- [ ] **Step 2: Run the focused test and verify RED**

Run: `cargo test terminal::view::send_sync_tests::reading_view_is_send_sync --no-default-features`

Expected: compilation fails because `dyn Book`, `dyn Container`, `dyn TerminalRender`, and the EPUB `RefCell<ZipArchive<_>>` are not `Send + Sync`.

- [ ] **Step 3: Put thread-safety requirements on the existing seams**

Use these supertraits:

```rust
pub trait Book: Send + Sync { /* existing methods unchanged */ }
pub(crate) trait Loader: Send + Sync { /* existing methods unchanged */ }
pub trait ContainerLoader: Send + Sync { /* existing methods unchanged */ }
pub trait Container: Send + Sync { /* existing methods unchanged */ }
pub(super) trait TerminalRender: Render<RenderContext> + Send + Sync {
    /* existing methods unchanged */
}
```

Change the private EPUB archive seam to return errors for every lock-taking operation:

```rust
trait EpubArchive: Send + Sync {
    fn is_encrypted(&self) -> Result<bool>;
    fn content(&self, path: &str) -> Result<Vec<u8>>;
    fn string(&self, path: &str) -> Result<String> {
        Ok(String::from_utf8(self.content(path)?)?)
    }
    fn exists(&self, path: &str) -> Result<bool>;
}
```

- [ ] **Step 4: Replace EPUB `RefCell` with `Mutex` and propagate errors**

Use `std::sync::{Mutex, MutexGuard}` and define:

```rust
struct EpubZipArchive<R: Read + Seek + Send> {
    zip: Mutex<ZipArchive<R>>,
}

impl<R: Read + Seek + Send> EpubZipArchive<R> {
    fn zip(&self) -> Result<MutexGuard<'_, ZipArchive<R>>> {
        self.zip
            .lock()
            .map_err(|_| anyhow!("EPUB archive lock poisoned"))
    }
}
```

Implement `is_encrypted`, `content`, and `exists` through `self.zip()?`. Return `Ok(...)` from `EpubExtractedArchive`. Update `EpubBook::new` to call `archive.is_encrypted()?`.

Change `setup_spine` to return `Result<(Spine, Option<String>)>` and use `archive.exists(&item.href)?`; propagate it from `setup_content_opf` with `?`. Do not add `unsafe impl Send` or `unsafe impl Sync`.

- [ ] **Step 5: Verify GREEN across both frontends**

Run: `cargo test terminal::view::send_sync_tests::reading_view_is_send_sync --no-default-features`

Expected: the compile-time test passes.

Run: `cargo test --all-features`

Expected: all tests pass and GTK types remain on the main thread.

- [ ] **Step 6: Commit the safe thread contract**

```powershell
git add src/book.rs src/container.rs src/book/epub.rs src/terminal/view.rs
git commit -m "Make terminal reader state thread safe"
```

### Task 4: Upgrade Cursive and delete the local forks

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Delete: `vendor/cursive-core/`
- Delete: `vendor/safer-owning-ref/`

- [ ] **Step 1: Switch both platform declarations to Cursive 0.21.1**

In `Cargo.toml`, replace both Cursive declarations with:

```toml
cursive = { version = "0.21.1", default-features = false, features = ["crossterm-backend", "toml"] }
```

Delete the entire `[patch.crates-io]` section and its two comments.

- [ ] **Step 2: Remove the obsolete vendor trees and resolve the lockfile**

Verify the resolved targets are exactly `D:\tbr-plus\vendor\cursive-core` and `D:\tbr-plus\vendor\safer-owning-ref`, then run:

```powershell
git rm -r vendor/cursive-core vendor/safer-owning-ref
cargo update -p cursive -p cursive_core
```

Expected: Cargo resolves Cursive 0.21.1 and registry `cursive_core` 0.4.x; `owning_ref` disappears.

- [ ] **Step 3: Verify the prepared source against Cursive 0.21**

Run: `cargo check --all-features`

Expected: exit 0 because Task 3 already satisfies `View: Send + Sync` and the source already imports `cursive::CursiveExt`. If another Cursive API error appears, stop this task and update the plan from that concrete compiler evidence; do not make speculative shortcut, layout, input-method, or rendering changes.

- [ ] **Step 4: Verify the dependency and behavior gates**

Run: `cargo tree -i owning_ref`

Expected: Cargo reports that `owning_ref` does not match any package.

Run: `cargo test --all-features`

Expected: all tests pass.

Run: `cargo build --no-default-features`

Expected: terminal binary builds without vendored-code warnings.

- [ ] **Step 5: Commit the upstream migration**

```powershell
git add Cargo.toml Cargo.lock
git commit -m "Upgrade terminal UI to Cursive 0.21"
```

### Task 5: Add deterministic characterization fixtures

**Files:**
- Create: `tests/fixtures/reader_sample.html`
- Create: `tests/fixtures/minimal_epub/META-INF/container.xml`
- Create: `tests/fixtures/minimal_epub/OEBPS/content.opf`
- Create: `tests/fixtures/minimal_epub/OEBPS/toc.ncx`
- Create: `tests/fixtures/minimal_epub/OEBPS/chapter.xhtml`
- Modify: `src/html_parser.rs`
- Modify: `src/book/epub.rs`
- Modify: `src/terminal/view/xi.rs`
- Modify: `src/terminal/view/han.rs`
- Modify: `src/gui.rs`

- [ ] **Step 1: Add the HTML/CSS fixture and an intentionally incomplete expectation**

Create `tests/fixtures/reader_sample.html`:

```html
<!doctype html>
<html>
  <head>
    <title>Fixture</title>
    <style>p { color: #123456; } a { text-decoration: underline; }</style>
  </head>
  <body>
    <h1 id="chapter">第一章</h1>
    <p>阅读<a href="next.xhtml#start">链接</a></p>
  </body>
</html>
```

Add this test module to `src/html_parser.rs`, starting with the deliberately empty line expectation:

```rust
#[cfg(test)]
mod tests {
    use super::{HtmlParseOptions, parse};

    #[test]
    fn fixture_preserves_title_text_and_link() {
        let html = include_str!("../tests/fixtures/reader_sample.html");
        let (content, font_faces) = parse(HtmlParseOptions::new(html)).unwrap();
        let lines = content
            .lines()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        assert!(font_faces.is_empty());
        assert_eq!(content.title(), Some("Fixture"));
        assert!(content.id_position("chapter").is_some());
        assert_eq!(lines, Vec::<String>::new());
        let target = content.lines()[1]
            .link_iter(true, |link| (true, Some(link.target.to_owned())));
        assert_eq!(target.as_deref(), Some("next.xhtml#start"));
    }
}
```

- [ ] **Step 2: Verify HTML characterization RED, then record the stable domain result**

Run: `cargo test html_parser::tests::fixture_preserves_title_text_and_link --all-features -- --exact`

Expected: failure showing the actual non-empty line vector.

Replace the empty expectation with `vec!["第一章", "阅读链接"]`; assert `content.title() == Some("Fixture")`, `content.id_position("chapter").is_some()`, and use `link_iter` to assert the target is `next.xhtml#start`.

- [ ] **Step 3: Add the minimal extracted EPUB fixture**

Create `tests/fixtures/minimal_epub/META-INF/container.xml`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<container>
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
```

Create `tests/fixtures/minimal_epub/OEBPS/content.opf`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<package version="2.0">
  <metadata>
    <title>Fixture EPUB</title>
    <language>zh-CN</language>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx"><itemref idref="chapter"/></spine>
</package>
```

Create `tests/fixtures/minimal_epub/OEBPS/toc.ncx`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<ncx>
  <navMap>
    <navPoint id="chapter" playOrder="1">
      <navLabel><text>第一章</text></navLabel>
      <content src="chapter.xhtml#start"/>
    </navPoint>
  </navMap>
</ncx>
```

Create `tests/fixtures/minimal_epub/OEBPS/chapter.xhtml`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<html><head><title>第一章</title></head><body><h1 id="start">第一章</h1><p>本地优先阅读。</p></body></html>
```

Add this initial RED test to `src/book/epub.rs`:

```rust
#[cfg(test)]
mod tests {
    use anyhow::Result;

    use crate::book::{BookContent, BookLoader, LoadingChapter};
    use crate::config::BookLoadingInfo;

    #[test]
    fn minimal_extracted_epub_loads_through_book_loader() -> Result<()> {
        let filename = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/minimal_epub/META-INF/container.xml")
            .to_string_lossy()
            .replace('\\', "/");
        let (book, _) = BookLoader::default().load(
            &filename,
            BookContent::File(filename.clone()),
            LoadingChapter::Index(0),
            BookLoadingInfo::NewReading(&filename, 0, 0, 16),
        )?;

        assert_eq!(book.name(), Some("wrong"));
        Ok(())
    }
}
```

Initially assert `book.name() == Some("wrong")`.

- [ ] **Step 4: Verify EPUB characterization RED, then record behavior**

Run: `cargo test book::epub::tests::minimal_extracted_epub_loads_through_book_loader --all-features -- --exact`

Expected: assertion failure showing `Fixture EPUB`.

Replace the final assertion block with:

```rust
assert_eq!(book.name(), Some("Fixture EPUB"));
assert_eq!(book.chapter_count(), 1);
let text = book.lines().iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
assert!(text.contains("第一章"));
assert!(text.contains("本地优先阅读。"));
let first_toc = book.toc_iterator().unwrap().next().unwrap();
assert_eq!(first_toc.title, "第一章");
```

- [ ] **Step 5: Add horizontal and vertical semantic layout tests**

In the existing test module in `src/terminal/view/xi.rs`, add:

```rust
fn rendered_rows(context: &RenderContext) -> Vec<String> {
    context
        .print_lines
        .iter()
        .map(|row| row.iter().map(|draw| draw.char).collect())
        .collect()
}

#[test]
fn horizontal_layout_preserves_cjk_order() {
    let lines = vec![Line::new("甲乙丙丁")];
    let book = DummyBook { lines };
    let mut context = RenderContext {
        width: 6,
        height: 2,
        print_lines: vec![],
        leading_space: 0,
    };
    let mut xi = Xi::new();

    xi.redraw(&book, book.lines(), 0, 0, &None, &mut context);

    assert_eq!(rendered_rows(&context), Vec::<String>::new());
}
```

Run the focused test, confirm RED, then replace the last assertion with `assert_eq!(rendered_rows(&context), vec!["甲乙丙", "丁    "]);`.

Append this test module to `src/terminal/view/han.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::Han;
    use crate::book::{Book, Line};
    use crate::controller::Render;
    use crate::terminal::view::RenderContext;

    struct DummyBook {
        lines: Vec<Line>,
    }

    impl Book for DummyBook {
        fn lines(&self) -> &Vec<Line> {
            &self.lines
        }
    }

    #[test]
    fn vertical_layout_preserves_cjk_order() {
        let book = DummyBook {
            lines: vec![Line::new("甲乙丙丁")],
        };
        let mut context = RenderContext {
            width: 5,
            height: 3,
            print_lines: vec![],
            leading_space: 0,
        };
        let mut han = Han::new();

        han.redraw(&book, book.lines(), 0, 0, &None, &mut context);
        let rows = context
            .print_lines
            .iter()
            .map(|row| row.iter().map(|draw| draw.char).collect::<String>())
            .collect::<Vec<_>>();

        assert_eq!(rows, Vec::<String>::new());
    }
}
```

Run the focused test, confirm RED, then replace the last assertion with `assert_eq!(rows, vec!["丁 甲", "  乙", "  丙"]);`. These tests compare character order and spacing, not screenshots or platform fonts.

- [ ] **Step 6: Reuse embedded SVG assets for a parser/render smoke test**

Append this module to `src/gui.rs`; no new SVG file or dependency is needed:

```rust
#[cfg(test)]
mod tests {
    use super::load_icons;

    #[test]
    fn embedded_svg_icons_render() {
        let icons = load_icons();
        assert!(icons.contains_key("toc.svg"));
        assert!(icons.contains_key("search.svg"));
        assert!(icons.contains_key("chapter.svg"));
    }
}
```

Run: `cargo test gui::tests::embedded_svg_icons_render --all-features -- --exact`

Expected: 1 test passes without opening a display.

- [ ] **Step 7: Run and commit the characterization suite**

Run: `cargo test --all-features`

Expected: all old and new tests pass.

```powershell
git add tests/fixtures src/html_parser.rs src/book/epub.rs src/terminal/view/xi.rs src/terminal/view/han.rs src/gui.rs
git commit -m "Add reader format regression fixtures"
```

### Task 6: Enforce the new boundaries in CI and run acceptance

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add library/no-default checks**

After the all-feature test step, add:

```yaml
      - run: cargo test --lib --no-default-features
      - run: cargo build --no-default-features
        env:
          RUSTFLAGS: -D warnings
```

Remove the old duplicate `cargo build --no-default-features` step.

- [ ] **Step 2: Run the complete acceptance suite**

Run in order:

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --lib --no-default-features
$env:RUSTFLAGS = "-D warnings"; cargo build --no-default-features; Remove-Item Env:RUSTFLAGS
cargo audit --no-fetch
cargo tree -i owning_ref
git diff --check
git status --short
```

Expected: format, Clippy, tests, warning-denied terminal build, audit, and diff checks exit 0; `cargo tree -i owning_ref` reports no matching package; status lists only the intended CI change plus the pre-existing untracked `docs/research/` directory.

- [ ] **Step 3: Commit the CI gate**

```powershell
git add .github/workflows/ci.yml
git commit -m "Enforce library and terminal build gates"
```

- [ ] **Step 4: Record final evidence**

Run: `git log --oneline -7`

Expected: the design commit followed by six focused implementation commits. Report test counts, audit result, Cursive/cursive_core versions, and the unchanged `docs/research/` state.
