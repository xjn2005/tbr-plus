use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

use super::anchor::Anchor;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BookId([u8; 32]);

impl BookId {
    pub fn from_file(path: &Path, title: &str, author: Option<&str>) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(Self::finish_hash(hasher, title, author))
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: [u8; 32] = bytes.try_into().ok()?;
        Some(Self(bytes))
    }

    fn finish_hash(mut hasher: Sha256, title: &str, author: Option<&str>) -> Self {
        hasher.update([0]);
        hasher.update(normalize_metadata(title));
        hasher.update([0]);
        if let Some(author) = author {
            hasher.update(normalize_metadata(author));
        }
        Self(hasher.finalize().into())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnnotationColor {
    Yellow,
    Blue,
    Green,
    Red,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnnotationStatus {
    Normal,
    Pending,
}

impl AnnotationStatus {
    pub(crate) fn as_db_value(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Pending => "pending",
        }
    }

    pub(crate) fn from_db_value(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "pending" => Some(Self::Pending),
            _ => None,
        }
    }
}

impl AnnotationColor {
    pub(crate) fn as_db_value(self) -> &'static str {
        match self {
            Self::Yellow => "yellow",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Red => "red",
        }
    }

    pub(crate) fn from_db_value(value: &str) -> Option<Self> {
        match value {
            "yellow" => Some(Self::Yellow),
            "blue" => Some(Self::Blue),
            "green" => Some(Self::Green),
            "red" => Some(Self::Red),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NewAnnotation {
    pub anchor: Anchor,
    pub color: AnnotationColor,
    pub note: Option<String>,
    pub tags: Vec<String>,
    pub starred: bool,
    pub status: AnnotationStatus,
}

impl NewAnnotation {
    pub fn new(
        anchor: Anchor,
        color: AnnotationColor,
        note: Option<String>,
        tags: Vec<String>,
    ) -> Self {
        let mut tags: Vec<String> = tags
            .into_iter()
            .map(|tag| tag.trim().trim_start_matches('#').to_owned())
            .filter(|tag| !tag.is_empty())
            .collect();
        tags.sort();
        tags.dedup();
        Self {
            anchor,
            color,
            note: note.filter(|note| !note.trim().is_empty()),
            tags,
            starred: false,
            status: AnnotationStatus::Normal,
        }
    }

    pub fn starred(mut self, starred: bool) -> Self {
        self.starred = starred;
        self
    }

    pub fn status(mut self, status: AnnotationStatus) -> Self {
        self.status = status;
        self
    }
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
pub struct BookMetadata {
    pub title: String,
    pub author: Option<String>,
}

impl BookMetadata {
    pub fn new(title: impl Into<String>, author: Option<impl Into<String>>) -> Self {
        Self {
            title: title.into(),
            author: author.map(Into::into),
        }
    }
}

fn normalize_metadata(value: &str) -> String {
    value
        .split_whitespace()
        .flat_map(str::chars)
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::BookId;

    #[test]
    fn book_id_uses_content_and_normalized_metadata_not_path() {
        let first = BookId::from_content(b"book bytes", "  A   Book  ", Some(" Ada "));
        let second = BookId::from_content(b"book bytes", "a book", Some("ada"));
        assert_eq!(first, second);
    }

    #[test]
    fn book_id_changes_when_content_or_metadata_changes() {
        let original = BookId::from_content(b"book bytes", "A Book", Some("Ada"));
        assert_ne!(
            original,
            BookId::from_content(b"other bytes", "A Book", Some("Ada"))
        );
        assert_ne!(
            original,
            BookId::from_content(b"book bytes", "Another Book", Some("Ada"))
        );
    }

    #[test]
    fn book_id_from_file_matches_the_same_content_identity() {
        let path = std::env::temp_dir().join("tbr-plus-book-id-test.txt");
        std::fs::write(&path, "一本值得重读的书").unwrap();
        let from_file = BookId::from_file(&path, "A Book", Some("Ada")).unwrap();
        let from_content =
            BookId::from_content("一本值得重读的书".as_bytes(), "A Book", Some("Ada"));
        assert_eq!(from_file, from_content);
        std::fs::remove_file(path).unwrap();
    }
}
