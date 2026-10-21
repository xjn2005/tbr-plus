use std::path::Path;

use anyhow::Result;

use crate::annotations::{Annotation, BookId};
use crate::config::BookLoadingInfo;
use crate::container::{ContainerManager, load_book, load_container};

use super::LibraryDocument;

pub(crate) fn collect_file_documents(
    path: &Path,
    book_id: &BookId,
    book_title: &str,
    annotations: &[Annotation],
) -> Result<Vec<LibraryDocument>> {
    let filename = path.to_string_lossy().into_owned();
    let container_manager = ContainerManager::default();
    let mut container = load_container(&container_manager, &filename)?;
    let inner_book_count = container.inner_book_names().map_or(1, Vec::len);
    let mut documents = vec![];

    for inner_book in 0..inner_book_count {
        let title = if inner_book_count == 1 {
            book_title.to_owned()
        } else {
            container.book_name(inner_book).to_owned()
        };
        let (mut book, _) = load_book(
            &container_manager,
            &mut container,
            BookLoadingInfo::NewReading(&filename, inner_book, 0, 20),
        )?;
        for chapter_index in 0..book.chapter_count() {
            if chapter_index > 0 {
                book.goto_chapter(chapter_index)?;
            }
            let chapter = format!("第 {} 章", chapter_index + 1);
            documents.extend(documents_for_chapter(
                book.lines(),
                book_id,
                &title,
                &filename,
                &chapter,
                inner_book,
                chapter_index,
                annotations,
            ));
        }
    }
    Ok(documents)
}

#[allow(clippy::too_many_arguments)]
fn documents_for_chapter(
    lines: &[crate::book::Line],
    book_id: &BookId,
    book_title: &str,
    source_path: &str,
    chapter: &str,
    inner_book: usize,
    chapter_index: usize,
    annotations: &[Annotation],
) -> Vec<LibraryDocument> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(line, text)| {
            let body = text.to_string();
            if body.trim().is_empty() {
                return None;
            }
            let related: Vec<_> = annotations
                .iter()
                .filter(|annotation| {
                    annotation.anchor.inner_book == inner_book
                        && annotation.anchor.chapter == chapter_index
                        && annotation.anchor.line <= line
                        && line <= annotation.anchor.end_line
                })
                .collect();
            let note = related
                .iter()
                .filter_map(|annotation| annotation.note.as_deref())
                .collect::<Vec<_>>()
                .join("\n");
            let tags = related
                .iter()
                .flat_map(|annotation| annotation.tags.iter().cloned())
                .collect();
            Some(LibraryDocument {
                book_id: book_id.clone(),
                book_title: book_title.to_owned(),
                source_path: source_path.to_owned(),
                chapter: chapter.to_owned(),
                inner_book,
                chapter_index,
                line,
                body,
                note: (!note.is_empty()).then_some(note),
                tags,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::annotations::BookId;

    use super::collect_file_documents;

    #[test]
    fn indexes_every_nonempty_line_from_a_local_text_file() {
        let path = std::env::temp_dir().join("tbr-plus-library-indexer.txt");
        std::fs::write(&path, "第一行\n\nsecond line\n").unwrap();
        let book_id = BookId::from_content(b"book", "Notes", None);

        let documents = collect_file_documents(&path, &book_id, "Notes", &[]).unwrap();

        assert_eq!(documents.len(), 2);
        assert_eq!(documents[0].body, "第一行");
        assert_eq!(documents[1].body, "second line");
        assert_eq!(documents[1].source_path, path.to_string_lossy());
        let _ = std::fs::remove_file(path);
    }
}
