use std::ops::Range;

use crate::book::Line;

const CONTEXT_CHARS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Anchor {
    pub inner_book: usize,
    pub chapter: usize,
    pub line: usize,
    pub start_char: usize,
    pub end_line: usize,
    pub end_char: usize,
    quote: String,
    prefix: String,
    suffix: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedAnchor {
    pub line: usize,
    pub range: Range<usize>,
}

impl ResolvedAnchor {
    #[inline]
    pub fn new(line: usize, range: Range<usize>) -> Self {
        Self { line, range }
    }
}

impl Anchor {
    pub fn new(
        inner_book: usize,
        chapter: usize,
        line: usize,
        start_char: usize,
        end_line: usize,
        end_char: usize,
        lines: &[Line],
    ) -> Option<Self> {
        if line > end_line {
            return None;
        }
        let text = lines.get(line)?;
        let end_text = lines.get(end_line)?;
        let end_char = usize::min(end_char, end_text.len());
        if start_char > text.len() || (line == end_line && start_char >= end_char) {
            return None;
        }
        Some(Self {
            inner_book,
            chapter,
            line,
            start_char,
            end_line,
            end_char,
            quote: selected_chars(lines, line, start_char, end_line, end_char)?,
            prefix: line_chars(text, start_char.saturating_sub(CONTEXT_CHARS)..start_char)?,
            suffix: line_chars(
                end_text,
                end_char..usize::min(end_char + CONTEXT_CHARS, end_text.len()),
            )?,
        })
    }

    #[inline]
    pub fn quote(&self) -> &str {
        &self.quote
    }

    #[inline]
    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }

    #[inline]
    pub(crate) fn suffix(&self) -> &str {
        &self.suffix
    }

    pub(crate) fn from_parts(
        inner_book: usize,
        chapter: usize,
        line: usize,
        start_char: usize,
        end_line: usize,
        end_char: usize,
        quote: String,
        prefix: String,
        suffix: String,
    ) -> Self {
        Self {
            inner_book,
            chapter,
            line,
            start_char,
            end_line,
            end_char,
            quote,
            prefix,
            suffix,
        }
    }

    pub fn reconnect(&self, lines: &[Line]) -> Option<ResolvedAnchor> {
        let exact = format!("{}{}{}", self.prefix, self.quote, self.suffix);
        let contextual = find_matches(
            lines,
            &exact,
            self.prefix.chars().count(),
            self.quote.chars().count(),
        );
        if contextual.len() == 1 {
            return contextual.into_iter().next();
        }
        let quotes = find_matches(lines, &self.quote, 0, self.quote.chars().count());
        if quotes.len() == 1 {
            quotes.into_iter().next()
        } else {
            None
        }
    }

    pub fn ranges(&self, lines: &[Line]) -> Option<Vec<ResolvedAnchor>> {
        let start = lines.get(self.line)?;
        let end = lines.get(self.end_line)?;
        if self.line > self.end_line || self.start_char > start.len() || self.end_char > end.len() {
            return None;
        }
        let mut ranges = vec![];
        for line in self.line..=self.end_line {
            let text = &lines[line];
            let start = if line == self.line {
                self.start_char
            } else {
                0
            };
            let end = if line == self.end_line {
                self.end_char
            } else {
                text.len()
            };
            if start < end {
                ranges.push(ResolvedAnchor::new(line, start..end));
            }
        }
        Some(ranges)
    }
}

fn selected_chars(
    lines: &[Line],
    start_line: usize,
    start_char: usize,
    end_line: usize,
    end_char: usize,
) -> Option<String> {
    let mut quote = String::new();
    for line in start_line..=end_line {
        if line != start_line {
            quote.push('\n');
        }
        let text = lines.get(line)?;
        let start = if line == start_line { start_char } else { 0 };
        let end = if line == end_line {
            end_char
        } else {
            text.len()
        };
        quote.push_str(&line_chars(text, start..end)?);
    }
    Some(quote)
}

fn line_chars(line: &Line, range: Range<usize>) -> Option<String> {
    if range.start > range.end || range.end > line.len() {
        return None;
    }
    let mut text = String::new();
    for index in range {
        text.push(line.char_at(index)?);
    }
    Some(text)
}

fn find_matches(
    lines: &[Line],
    needle: &str,
    quote_offset: usize,
    quote_len: usize,
) -> Vec<ResolvedAnchor> {
    if needle.is_empty() {
        return vec![];
    }
    let needle: Vec<char> = needle.chars().collect();
    let mut text = vec![];
    let mut line_starts = vec![];
    for (line_index, line) in lines.iter().enumerate() {
        line_starts.push(text.len());
        text.extend((0..line.len()).filter_map(|index| line.char_at(index)));
        if line_index + 1 < lines.len() {
            text.push('\n');
        }
    }
    let mut matches = vec![];
    if needle.len() > text.len() {
        return matches;
    }
    for start in 0..=text.len() - needle.len() {
        if !needle
            .iter()
            .enumerate()
            .all(|(offset, ch)| text[start + offset] == *ch)
        {
            continue;
        }
        let quote_start = start + quote_offset;
        let Some((line_index, line_start)) = line_starts
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line_start)| quote_start >= **line_start)
            .map(|(line_index, line_start)| (line_index, *line_start))
        else {
            continue;
        };
        let line = &lines[line_index];
        let start_char = quote_start - line_start;
        if start_char < line.len() {
            matches.push(ResolvedAnchor::new(
                line_index,
                start_char..usize::min(start_char + quote_len, line.len()),
            ));
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use crate::book::Line;

    use super::{Anchor, ResolvedAnchor};

    #[test]
    fn anchor_preserves_cjk_character_offsets() {
        let lines = vec![Line::new("你好，世界")];
        let anchor = Anchor::new(0, 0, 0, 1, 0, 4, &lines).unwrap();
        assert_eq!(anchor.quote(), "好，世");
    }

    #[test]
    fn reconnect_uses_context_to_choose_duplicate_quote() {
        let original = vec![Line::new("甲 shared 乙")];
        let anchor = Anchor::new(0, 0, 0, 2, 0, 8, &original).unwrap();
        let changed = vec![Line::new("丙 shared 丁"), Line::new("甲 shared 乙")];
        assert_eq!(
            anchor.reconnect(&changed),
            Some(ResolvedAnchor::new(1, 2..8)),
        );
    }

    #[test]
    fn anchor_preserves_a_selection_across_lines() {
        let lines = vec![Line::new("第一行文字"), Line::new("第二行文字")];
        let anchor = Anchor::new(0, 0, 0, 2, 1, 3, &lines).unwrap();
        assert_eq!(anchor.quote(), "行文字\n第二行");
        assert_eq!(
            anchor.ranges(&lines),
            Some(vec![
                ResolvedAnchor::new(0, 2..5),
                ResolvedAnchor::new(1, 0..3)
            ])
        );
    }

    #[test]
    fn reconnect_restores_a_multi_line_selection_after_an_inserted_line() {
        let original = vec![Line::new("第一行文字"), Line::new("第二行文字")];
        let anchor = Anchor::new(0, 0, 0, 2, 1, 3, &original).unwrap();
        let changed = vec![
            Line::new("引言"),
            Line::new("第一行文字"),
            Line::new("第二行文字"),
        ];
        assert_eq!(
            anchor.reconnect(&changed),
            Some(ResolvedAnchor::new(1, 2..5)),
        );
    }
}
