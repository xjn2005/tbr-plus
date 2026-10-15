use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use rusqlite::{params, Connection, Params, Transaction};

use super::anchor::Anchor;
use super::model::{Annotation, AnnotationColor, AnnotationStatus, BookId, NewAnnotation};

const DATABASE_VERSION: i64 = 3;

pub struct AnnotationStore {
    connection: Connection,
}

impl AnnotationStore {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch("pragma foreign_keys = on;")?;
        connection.execute_batch(
            "
create table if not exists info (version integer not null);
create table if not exists books (
    book_id blob primary key,
    title text not null,
    author text,
    last_path text not null,
    created_at integer not null,
    updated_at integer not null
);
create table if not exists annotations (
    annotation_id integer primary key,
    book_id blob not null references books(book_id) on delete cascade,
    inner_book integer not null,
    chapter integer not null,
    line integer not null,
    start_char integer not null,
    end_line integer not null,
    end_char integer not null,
    quote text not null,
    prefix text not null,
    suffix text not null,
    color text not null,
    note text,
    starred integer not null default 0,
    status text not null default 'normal',
    created_at integer not null,
    updated_at integer not null
);
create table if not exists tags (
    tag_id integer primary key,
    name text not null unique
);
create table if not exists annotation_tags (
    annotation_id integer not null references annotations(annotation_id) on delete cascade,
    tag_id integer not null references tags(tag_id) on delete cascade,
    primary key (annotation_id, tag_id)
);
create index if not exists annotations_by_location
on annotations(book_id, inner_book, chapter, line);
",
        )?;
        let version: Option<i64> = connection
            .query_row("select version from info limit 1", [], |row| row.get(0))
            .ok();
        match version {
            None => {
                connection.execute("insert into info (version) values (?)", [DATABASE_VERSION])?;
            }
            Some(1) => {
                connection.execute_batch(
                    "alter table annotations add column end_line integer not null default 0;
                     update annotations set end_line = line where end_line = 0;
                     update info set version = 2;",
                )?;
            }
            Some(2) => {
                connection.execute_batch(
                    "alter table annotations add column starred integer not null default 0;
                     alter table annotations add column status text not null default 'normal';
                     update info set version = 3;",
                )?;
            }
            Some(DATABASE_VERSION) => {}
            Some(version) => bail!("Unsupported annotation database version: {version}"),
        }
        Ok(Self { connection })
    }

    pub fn upsert_book(
        &mut self,
        book_id: &BookId,
        title: &str,
        author: Option<&str>,
        path: &str,
    ) -> Result<()> {
        let now = timestamp();
        self.connection.execute(
            "
insert into books (book_id, title, author, last_path, created_at, updated_at)
values (?, ?, ?, ?, ?, ?)
on conflict(book_id) do update set
    title = excluded.title,
    author = excluded.author,
    last_path = excluded.last_path,
    updated_at = excluded.updated_at
",
            params![book_id.as_bytes().as_slice(), title, author, path, now, now],
        )?;
        Ok(())
    }

    pub fn create(&mut self, book_id: &BookId, annotation: NewAnnotation) -> Result<i64> {
        let now = timestamp();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "
insert into annotations (
    book_id, inner_book, chapter, line, start_char, end_line, end_char,
    quote, prefix, suffix, color, note, starred, status, created_at, updated_at
) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
",
            params![
                book_id.as_bytes().as_slice(),
                annotation.anchor.inner_book as i64,
                annotation.anchor.chapter as i64,
                annotation.anchor.line as i64,
                annotation.anchor.start_char as i64,
                annotation.anchor.end_line as i64,
                annotation.anchor.end_char as i64,
                annotation.anchor.quote(),
                annotation.anchor.prefix(),
                annotation.anchor.suffix(),
                annotation.color.as_db_value(),
                annotation.note,
                annotation.starred as i64,
                annotation.status.as_db_value(),
                now,
                now,
            ],
        )?;
        let annotation_id = transaction.last_insert_rowid();
        store_tags(&transaction, annotation_id, &annotation.tags)?;
        transaction.commit()?;
        Ok(annotation_id)
    }

    pub fn update(&mut self, annotation_id: i64, annotation: NewAnnotation) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "
update annotations set
    inner_book = ?, chapter = ?, line = ?, start_char = ?, end_line = ?, end_char = ?,
    quote = ?, prefix = ?, suffix = ?, color = ?, note = ?, starred = ?, status = ?, updated_at = ?
where annotation_id = ?
",
            params![
                annotation.anchor.inner_book as i64,
                annotation.anchor.chapter as i64,
                annotation.anchor.line as i64,
                annotation.anchor.start_char as i64,
                annotation.anchor.end_line as i64,
                annotation.anchor.end_char as i64,
                annotation.anchor.quote(),
                annotation.anchor.prefix(),
                annotation.anchor.suffix(),
                annotation.color.as_db_value(),
                annotation.note,
                annotation.starred as i64,
                annotation.status.as_db_value(),
                timestamp(),
                annotation_id,
            ],
        )?;
        if updated != 1 {
            bail!("Annotation {annotation_id} does not exist");
        }
        transaction.execute(
            "delete from annotation_tags where annotation_id = ?",
            [annotation_id],
        )?;
        store_tags(&transaction, annotation_id, &annotation.tags)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn delete(&mut self, annotation_id: i64) -> Result<()> {
        let deleted = self.connection.execute(
            "delete from annotations where annotation_id = ?",
            [annotation_id],
        )?;
        if deleted != 1 {
            bail!("Annotation {annotation_id} does not exist");
        }
        Ok(())
    }

    pub fn set_starred(&mut self, annotation_id: i64, starred: bool) -> Result<()> {
        self.update_flag("starred", annotation_id, starred as i64)
    }

    pub fn set_status(&mut self, annotation_id: i64, status: AnnotationStatus) -> Result<()> {
        let updated = self.connection.execute(
            "update annotations set status = ?, updated_at = ? where annotation_id = ?",
            params![status.as_db_value(), timestamp(), annotation_id],
        )?;
        if updated != 1 {
            bail!("Annotation {annotation_id} does not exist");
        }
        Ok(())
    }

    pub fn list_for_book(&self, book_id: &BookId) -> Result<Vec<Annotation>> {
        self.list(
            "
select annotation_id, inner_book, chapter, line, start_char, end_line, end_char,
       quote, prefix, suffix, color, note, starred, status, created_at, updated_at
from annotations
where book_id = ?
order by created_at, annotation_id
	",
            [book_id.as_bytes().as_slice()],
        )
    }

    fn list<P: Params>(&self, query: &str, params: P) -> Result<Vec<Annotation>> {
        let mut statement = self.connection.prepare(query)?;
        let records = statement.query_map(params, |row| {
            let color: String = row.get(10)?;
            let color =
                AnnotationColor::from_db_value(&color).ok_or(rusqlite::Error::InvalidQuery)?;
            let status: String = row.get(13)?;
            let status =
                AnnotationStatus::from_db_value(&status).ok_or(rusqlite::Error::InvalidQuery)?;
            Ok(Annotation {
                id: row.get(0)?,
                anchor: Anchor::from_parts(
                    row.get::<_, i64>(1)? as usize,
                    row.get::<_, i64>(2)? as usize,
                    row.get::<_, i64>(3)? as usize,
                    row.get::<_, i64>(4)? as usize,
                    row.get::<_, i64>(5)? as usize,
                    row.get::<_, i64>(6)? as usize,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ),
                color,
                note: row.get(11)?,
                tags: vec![],
                starred: row.get::<_, i64>(12)? != 0,
                status,
                created_at: row.get(14)?,
            })
        })?;
        let mut annotations = vec![];
        for record in records {
            let mut annotation = record?;
            annotation.tags = self.list_tags(annotation.id)?;
            annotations.push(annotation);
        }
        Ok(annotations)
    }

    fn update_flag(&mut self, column: &str, annotation_id: i64, value: i64) -> Result<()> {
        let query =
            format!("update annotations set {column} = ?, updated_at = ? where annotation_id = ?");
        let updated = self
            .connection
            .execute(&query, params![value, timestamp(), annotation_id])?;
        if updated != 1 {
            bail!("Annotation {annotation_id} does not exist");
        }
        Ok(())
    }

    fn list_tags(&self, annotation_id: i64) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "
select tags.name
from tags join annotation_tags on annotation_tags.tag_id = tags.tag_id
where annotation_tags.annotation_id = ?
order by tags.name
",
        )?;
        let rows = statement.query_map([annotation_id], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
    }
}

fn store_tags(transaction: &Transaction, annotation_id: i64, tags: &[String]) -> Result<()> {
    for tag in tags {
        transaction.execute(
            "insert into tags (name) values (?) on conflict(name) do nothing",
            [tag],
        )?;
        let tag_id: i64 =
            transaction.query_row("select tag_id from tags where name = ?", [tag], |row| {
                row.get(0)
            })?;
        transaction.execute(
            "insert into annotation_tags (annotation_id, tag_id) values (?, ?)",
            [annotation_id, tag_id],
        )?;
    }
    Ok(())
}

fn timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::annotations::anchor::Anchor;
    use crate::annotations::model::{AnnotationColor, BookId, NewAnnotation};
    use crate::book::Line;

    use super::AnnotationStore;

    #[test]
    fn reopening_database_preserves_annotation_tags_and_order() {
        let path = std::env::temp_dir().join(format!(
            "tbr-plus-annotations-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let book_id = BookId::from_content(b"book", "A Book", Some("Ada"));
        let lines = vec![Line::new("first quote"), Line::new("second quote")];
        {
            let mut store = AnnotationStore::open(&path).unwrap();
            store
                .upsert_book(&book_id, "A Book", Some("Ada"), "C:/books/book.txt")
                .unwrap();
            store
                .create(
                    &book_id,
                    NewAnnotation::new(
                        Anchor::new(0, 0, 0, 0, 0, 5, &lines).unwrap(),
                        AnnotationColor::Yellow,
                        None,
                        vec!["systems".into()],
                    ),
                )
                .unwrap();
            store
                .create(
                    &book_id,
                    NewAnnotation::new(
                        Anchor::new(0, 0, 1, 0, 1, 6, &lines).unwrap(),
                        AnnotationColor::Blue,
                        Some("keep this".into()),
                        vec!["rust".into(), "systems".into()],
                    ),
                )
                .unwrap();
        }
        {
            let store = AnnotationStore::open(&path).unwrap();
            let annotations = store.list_for_book(&book_id).unwrap();
            assert_eq!(annotations.len(), 2);
            assert_eq!(annotations[0].anchor.quote(), "first");
            assert_eq!(annotations[1].tags, vec!["rust", "systems"]);
            assert_eq!(annotations[1].note.as_deref(), Some("keep this"));
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn update_replaces_annotation_fields_and_delete_removes_it() {
        let path = std::env::temp_dir().join(format!(
            "tbr-plus-annotations-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let book_id = BookId::from_content(b"book", "A Book", None);
        let lines = vec![Line::new("first quote"), Line::new("second quote")];
        let mut store = AnnotationStore::open(&path).unwrap();
        store
            .upsert_book(&book_id, "A Book", None, "C:/books/book.txt")
            .unwrap();
        let id = store
            .create(
                &book_id,
                NewAnnotation::new(
                    Anchor::new(0, 0, 0, 0, 0, 5, &lines).unwrap(),
                    AnnotationColor::Yellow,
                    None,
                    vec!["draft".into()],
                ),
            )
            .unwrap();
        store
            .update(
                id,
                NewAnnotation::new(
                    Anchor::new(0, 0, 1, 0, 1, 6, &lines).unwrap(),
                    AnnotationColor::Red,
                    Some("revised".into()),
                    vec!["rust".into(), "systems".into()],
                ),
            )
            .unwrap();
        let annotations = store.list_for_chapter(&book_id, 0, 0).unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].anchor.quote(), "second");
        assert_eq!(annotations[0].color, AnnotationColor::Red);
        assert_eq!(annotations[0].tags, vec!["rust", "systems"]);
        store.delete(id).unwrap();
        assert!(store.list_for_book(&book_id).unwrap().is_empty());
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}
