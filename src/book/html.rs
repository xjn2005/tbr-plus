use anyhow::Result;
use elsa::FrozenMap;
use indexmap::IndexSet;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;

use crate::book::{Book, ImageData, Line, Loader, LoadingChapter};
use crate::common::{TraceInfo, plain_text};
use crate::config::{BookLoadingInfo, ReadingInfo};
#[cfg(feature = "gui")]
use crate::gui::HtmlFonts;
#[cfg(feature = "gui")]
use crate::html_parser::BlockStyle;
use crate::html_parser::{HtmlContent, HtmlParseOptions, HtmlResolver};
use crate::xhtml::xhtml_to_html;
use crate::{frozen_map_get, html_parser};

pub(crate) struct HtmlLoader {
    extensions: Vec<&'static str>,
}

pub(crate) struct HtmlBook {
    path: Option<PathBuf>,
    content: HtmlContent,
    font_families: IndexSet<String>,
    #[cfg(feature = "gui")]
    fonts: HtmlFonts,
}

impl HtmlLoader {
    pub(crate) fn new() -> Self {
        let extensions = vec![".html", ".htm", ".xhtml", ".md", ".markdown"];
        HtmlLoader { extensions }
    }
}

struct HtmlContentResolver {
    cwd: PathBuf,
    css_cache: FrozenMap<String, String>,
    custom_style: Option<String>,
}

impl HtmlResolver for HtmlContentResolver {
    #[inline]
    fn cwd(&self) -> PathBuf {
        self.cwd.clone()
    }

    #[inline]
    fn resolve(&self, path: &PathBuf, sub: &str) -> PathBuf {
        path.join(sub)
    }

    fn css(&self, sub: &str) -> Option<(PathBuf, &str)> {
        let mut path = self.cwd.join(sub);
        let full_path = path.canonicalize().ok()?.to_str()?.to_string();
        let content = frozen_map_get!(&self.css_cache, full_path, || {
            fs::read_to_string(&path).ok()
        })?;
        path.pop();
        Some((path, content))
    }

    fn custom_style(&self) -> Option<&str> {
        self.custom_style.as_deref()
    }
}

impl Loader for HtmlLoader {
    fn extensions(&self) -> &Vec<&'static str> {
        &self.extensions
    }

    fn load_file(
        &self,
        _filename: &str,
        mut file: fs::File,
        _loading_chapter: LoadingChapter,
        loading: BookLoadingInfo,
    ) -> Result<(Box<dyn Book>, ReadingInfo)> {
        let filename = loading.filename();
        let mut cwd = PathBuf::from_str(filename)?;
        cwd.pop();
        let mut content: Vec<u8> = Vec::new();
        file.read_to_end(&mut content)?;
        let mut font_families = IndexSet::new();
        let mut text = plain_text(content, false)?;
        let lower_filename = filename.to_lowercase();
        if lower_filename.ends_with(".xhtml") {
            text = xhtml_to_html(&text)?;
        } else if lower_filename.ends_with(".md") || lower_filename.ends_with(".markdown") {
            text = markdown_to_html(&text);
        }
        let reading = get_reading(loading);
        #[allow(unused)]
        let (content, mut font_faces) = html_parser::parse(
            HtmlParseOptions::new(&text)
                .with_font_family(&mut font_families)
                .with_resolver(&HtmlContentResolver {
                    cwd: cwd.clone(),
                    css_cache: FrozenMap::new(),
                    custom_style: reading.custom_style.clone(),
                }),
        )?;
        #[cfg(feature = "gui")]
        let book = {
            let mut fonts = HtmlFonts::new();
            fonts.reload(font_faces, |path| {
                let content = fs::read(path).ok()?;
                Some(content)
            });
            HtmlBook {
                path: Some(cwd),
                content,
                font_families,
                fonts,
            }
        };
        #[cfg(not(feature = "gui"))]
        let book = HtmlBook {
            path: Some(cwd.to_owned()),
            content,
            font_families,
        };
        Ok((Box::new(book), reading))
    }

    fn load_buf(
        &self,
        _filename: &str,
        content: Vec<u8>,
        _loading_chapter: LoadingChapter,
        loading: BookLoadingInfo,
    ) -> Result<(Box<dyn Book>, ReadingInfo)> {
        let mut font_families = IndexSet::new();
        let text = plain_text(content, false)?;
        let (content, _) =
            html_parser::parse(HtmlParseOptions::new(&text).with_font_family(&mut font_families))?;
        let book = HtmlBook {
            path: None,
            content,
            font_families,
            #[cfg(feature = "gui")]
            fonts: HtmlFonts::new(),
        };
        let reading = get_reading(loading);
        Ok((Box::new(book), reading))
    }
}

fn markdown_to_html(markdown: &str) -> String {
    let mut html = String::new();
    let mut in_list = false;
    let mut in_code = false;
    for line in markdown.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("```") {
            if in_code {
                html.push_str("</code></pre>");
            } else {
                html.push_str("<pre><code>");
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            html.push_str(&html_escape(trimmed));
            html.push('\n');
            continue;
        }
        if trimmed.is_empty() {
            if in_list {
                html.push_str("</ul>");
                in_list = false;
            }
            continue;
        }
        if let Some((level, title)) = markdown_heading(trimmed) {
            if in_list {
                html.push_str("</ul>");
                in_list = false;
            }
            html.push_str(&format!("<h{level}>{}</h{level}>", inline_markdown(title)));
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            if !in_list {
                html.push_str("<ul>");
                in_list = true;
            }
            html.push_str(&format!("<li>{}</li>", inline_markdown(item)));
        } else if let Some(quote) = trimmed.strip_prefix("> ") {
            html.push_str(&format!(
                "<blockquote><p>{}</p></blockquote>",
                inline_markdown(quote)
            ));
        } else if trimmed.starts_with('|') && trimmed.ends_with('|') {
            let cells = trimmed
                .trim_matches('|')
                .split('|')
                .map(|cell| format!("<td>{}</td>", inline_markdown(cell.trim())))
                .collect::<String>();
            html.push_str(&format!("<table><tr>{cells}</tr></table>"));
        } else if !trimmed
            .chars()
            .all(|ch| ch == '|' || ch == '-' || ch == ':' || ch.is_whitespace())
        {
            if in_list {
                html.push_str("</ul>");
                in_list = false;
            }
            html.push_str(&format!("<p>{}</p>", inline_markdown(trimmed)));
        }
    }
    if in_list {
        html.push_str("</ul>");
    }
    if in_code {
        html.push_str("</code></pre>");
    }
    html
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|ch| *ch == '#').count();
    (1..=6)
        .contains(&level)
        .then(|| (level, line[level..].trim_start()))
}

fn inline_markdown(text: &str) -> String {
    let text = html_escape(text);
    let text = text.replace("**", "<strong>");
    let text = text.replace("__", "<strong>");
    text.replace('`', "<code>")
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl Book for HtmlBook {
    #[inline]
    fn name(&self) -> Option<&str> {
        self.content.title()
    }

    #[inline]
    fn lines(&self) -> &Vec<Line> {
        self.content.lines()
    }

    fn link_position(&mut self, line: usize, link_index: usize) -> Option<TraceInfo> {
        let text = &self.content.lines().get(line)?;
        let link = text.link_at(link_index)?;
        let link_target = link.target;
        let mut split = link_target.split('#');
        split.next()?;
        let anchor = split.next()?;
        let position = self.content.id_position(anchor)?;
        Some(TraceInfo {
            chapter: 0,
            line: position.line,
            offset: position.offset,
        })
    }

    fn image<'h>(&'h self, href: &'h str) -> Option<ImageData<'h>> {
        if let Some(path) = &self.path {
            let path = path.join(href);
            let bytes = fs::read(&path).ok()?;
            Some(ImageData::Owned((path.to_str()?.to_string(), bytes)))
        } else {
            None
        }
    }

    #[inline]
    fn font_family_names(&self) -> Option<&IndexSet<String>> {
        Some(&self.font_families)
    }

    #[cfg(feature = "gui")]
    #[inline]
    fn color_customizable(&self) -> bool {
        true
    }

    #[cfg(feature = "gui")]
    #[inline]
    fn fonts_customizable(&self) -> bool {
        true
    }

    #[cfg(feature = "gui")]
    #[inline]
    fn custom_fonts(&self) -> Option<&HtmlFonts> {
        if self.fonts.has_faces() {
            Some(&self.fonts)
        } else {
            None
        }
    }

    #[cfg(feature = "gui")]
    #[inline]
    fn style_customizable(&self) -> bool {
        true
    }

    #[cfg(feature = "gui")]
    #[inline]
    fn block_styles(&self) -> Option<&Vec<BlockStyle>> {
        self.content.block_styles()
    }
}

#[inline]
fn get_reading(loading: BookLoadingInfo) -> ReadingInfo {
    loading.get_or_init(|reading| {
        reading.custom_color = true;
        reading.custom_font = true;
    })
}

#[cfg(test)]
mod markdown_tests {
    use super::markdown_to_html;

    #[test]
    fn markdown_is_converted_to_controlled_html() {
        let html = markdown_to_html("# 标题\n\n- 一项\n- 二项\n\n> 引文\n\n```\nlet x = 1;\n```");
        assert!(html.contains("<h1>标题</h1>"));
        assert!(html.contains("<ul><li>一项</li><li>二项</li></ul>"));
        assert!(html.contains("<blockquote><p>引文</p></blockquote>"));
        assert!(html.contains("<pre><code>let x = 1;"));
    }
}
