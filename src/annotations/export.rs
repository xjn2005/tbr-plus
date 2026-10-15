use super::model::{Annotation, BookMetadata};

pub fn export_book(
    book: &BookMetadata,
    annotations: &[Annotation],
    with_front_matter: bool,
) -> String {
    let mut annotations: Vec<&Annotation> = annotations.iter().collect();
    annotations.sort_by_key(|annotation| {
        (
            annotation.anchor.chapter,
            annotation.anchor.line,
            annotation.anchor.start_char,
            annotation.created_at,
            annotation.id,
        )
    });
    let entries: Vec<String> = annotations.into_iter().map(export_annotation).collect();
    let body = format!("## 《{}》\n\n{}\n", book.title, entries.join("\n\n"));
    if with_front_matter {
        let mut front_matter = format!("---\ntitle: \"{}\"\n", yaml_string(&book.title));
        if let Some(author) = &book.author {
            front_matter.push_str(&format!("author: \"{}\"\n", yaml_string(author)));
        }
        front_matter.push_str("---\n\n");
        front_matter.push_str(&body);
        front_matter
    } else {
        body
    }
}

fn export_annotation(annotation: &Annotation) -> String {
    let quote = annotation
        .anchor
        .quote()
        .lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<String>>()
        .join("\n");
    let mut sections = vec![
        quote,
        format!("— Chapter {}", annotation.anchor.chapter + 1),
    ];
    if let Some(note) = &annotation.note {
        sections.push(note.clone());
    }
    if !annotation.tags.is_empty() {
        let mut tags = annotation.tags.clone();
        tags.sort();
        tags.dedup();
        sections.push(
            tags.into_iter()
                .map(|tag| format!("#{tag}"))
                .collect::<Vec<String>>()
                .join(" "),
        );
    }
    sections.join("\n\n")
}

fn yaml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use crate::annotations::anchor::Anchor;
    use crate::annotations::model::{
        Annotation, AnnotationColor, AnnotationStatus, BookId, BookMetadata,
    };
    use crate::book::Line;

    use super::export_book;

    #[test]
    fn markdown_export_is_stable_and_orders_annotations_by_location() {
        let lines = vec![Line::new("first quote"), Line::new("second quote")];
        let book_id = BookId::from_content(b"book", "A Book", Some("Ada"));
        let annotations = vec![
            Annotation {
                id: 2,
                book_id: book_id.clone(),
                anchor: Anchor::new(0, 1, 1, 0, 1, 6, &lines).unwrap(),
                color: AnnotationColor::Blue,
                note: Some("A thought".into()),
                tags: vec!["systems".into()],
                starred: false,
                status: AnnotationStatus::Normal,
                created_at: 2,
                updated_at: 2,
            },
            Annotation {
                id: 1,
                book_id,
                anchor: Anchor::new(0, 0, 0, 0, 0, 5, &lines).unwrap(),
                color: AnnotationColor::Yellow,
                note: None,
                tags: vec!["rust".into(), "systems".into()],
                starred: true,
                status: AnnotationStatus::Pending,
                created_at: 1,
                updated_at: 1,
            },
        ];
        let markdown = export_book(
            &BookMetadata::new("A Book", Some("Ada")),
            &annotations,
            true,
        );
        assert_eq!(
            markdown,
            concat!(
                "---\n",
                "title: \"A Book\"\n",
                "author: \"Ada\"\n",
                "---\n\n",
                "## 《A Book》\n\n",
                "> first\n\n",
                "— Chapter 1\n\n",
                "#rust #systems\n\n",
                "> second\n\n",
                "— Chapter 2\n\n",
                "A thought\n\n",
                "#systems\n",
            )
        );
    }
}
