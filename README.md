# tbr++

> 为值得重访的书而读：一个本地优先的中文阅读、摘录与检索工具。

[![CI](https://github.com/xjn2005/tbr-plus/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/xjn2005/tbr-plus/actions/workflows/ci.yml)
[![Rust 2021](https://img.shields.io/badge/Rust-2021-ea5b0c?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![许可证：GPL-2.0-only](https://img.shields.io/badge/许可证-GPL--2.0--only-5e5e5e?style=flat)](LICENSE)

## 适合你，如果……

- 你想读 EPUB、TXT、HTML、Haodoo 或 Markdown，并重视中文横排与竖排体验。
- 你希望高亮、批注、标签和阅读记录只留在自己的电脑上。
- 你想在读完之后，按正文、批注或标签重新找到一本书里的段落。

## 当前支持

- GTK 桌面阅读器与轻量终端阅读器。
- EPUB、TXT、HTML、Haodoo、Markdown。
- 高亮、批注、标签、收藏、待整理、Markdown 摘录导出与书库检索。

暂不支持 PDF、账号、云同步、移动端和 AI 总结。

## 快速开始

目前从源码运行。安装 Rust 与 GTK 4 开发环境后：

```powershell
git clone https://github.com/xjn2005/tbr-plus.git
cd tbr-plus
cargo run -- path\to\book.epub
```

## 怎么使用

1. 在桌面阅读器中选中一段文字，创建高亮并写下批注或标签。
2. 在“批注”页回看、收藏或标记待整理的摘录。
3. 导出 Markdown，或在“书库”中跨书检索正文、批注与标签。

## 本地优先

批注与书库索引保存在本机 SQLite 数据库中。tbr++ 不需要账号，不上传阅读内容，也不提供云同步。

## 许可证

[GPL-2.0-only](LICENSE)
