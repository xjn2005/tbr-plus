use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::annotations::BookId;

#[derive(Clone, Debug)]
pub struct LibraryDocument {
    pub book_id: BookId,
    pub book_title: String,
    pub source_path: String,
    pub chapter: String,
    pub inner_book: usize,
    pub chapter_index: usize,
    pub line: usize,
    pub body: String,
    pub note: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug)]
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
    pub tags: Vec<String>,
    pub score: f64,
}

pub struct LibraryIndex {
    connection: Connection,
}

impl LibraryIndex {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "
create table if not exists library_documents (
    document_id integer primary key,
    book_id blob not null,
    book_title text not null,
    source_path text not null default '',
    chapter text not null,
    inner_book integer not null default 0,
    chapter_index integer not null default 0,
    line integer not null,
    body text not null,
    note text,
    tags text not null
);
create virtual table if not exists library_fts using fts5(
    title, chapter, body, note, tags, tokenize='unicode61'
);
create index if not exists library_documents_book on library_documents(book_id);
",
        )?;
        ensure_column(
            &connection,
            "library_documents",
            "source_path",
            "text not null default ''",
        )?;
        ensure_column(
            &connection,
            "library_documents",
            "inner_book",
            "integer not null default 0",
        )?;
        ensure_column(
            &connection,
            "library_documents",
            "chapter_index",
            "integer not null default 0",
        )?;
        Ok(Self { connection })
    }

    pub fn replace_book(&mut self, book_id: &BookId, documents: &[LibraryDocument]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let old_ids = {
            let mut old = transaction
                .prepare("select document_id from library_documents where book_id = ?")?;
            let ids = old
                .query_map([book_id.as_bytes().as_slice()], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            ids
        };
        for id in old_ids {
            transaction.execute("delete from library_fts where rowid = ?", [id])?;
        }
        transaction.execute(
            "delete from library_documents where book_id = ?",
            [book_id.as_bytes().as_slice()],
        )?;
        for document in documents {
            let tags = document.tags.join(" ");
            transaction.execute(
                "insert into library_documents (book_id, book_title, source_path, chapter, inner_book, chapter_index, line, body, note, tags)
                 values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    document.book_id.as_bytes().as_slice(), document.book_title, document.source_path, document.chapter,
                    document.inner_book as i64, document.chapter_index as i64, document.line as i64, document.body, document.note, tags,
                ],
            )?;
            let id = transaction.last_insert_rowid();
            transaction.execute(
                "insert into library_fts(rowid, title, chapter, body, note, tags) values (?, ?, ?, ?, ?, ?)",
                params![
                    id,
                    searchable(&document.book_title),
                    searchable(&document.chapter),
                    searchable(&document.body),
                    document.note.as_deref().map(searchable),
                    searchable(&tags),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn replace_chapter(
        &mut self,
        book_id: &BookId,
        chapter: &str,
        documents: &[LibraryDocument],
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let old_ids = {
            let mut statement = transaction.prepare(
                "select document_id from library_documents where book_id = ? and chapter = ?",
            )?;
            let ids = statement
                .query_map(params![book_id.as_bytes().as_slice(), chapter], |row| {
                    row.get::<_, i64>(0)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            ids
        };
        for id in old_ids {
            transaction.execute("delete from library_fts where rowid = ?", [id])?;
        }
        transaction.execute(
            "delete from library_documents where book_id = ? and chapter = ?",
            params![book_id.as_bytes().as_slice(), chapter],
        )?;
        for document in documents {
            let tags = document.tags.join(" ");
            transaction.execute(
                "insert into library_documents (book_id, book_title, source_path, chapter, inner_book, chapter_index, line, body, note, tags)
                 values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![document.book_id.as_bytes().as_slice(), document.book_title, document.source_path, document.chapter, document.inner_book as i64, document.chapter_index as i64, document.line as i64, document.body, document.note, tags],
            )?;
            let id = transaction.last_insert_rowid();
            transaction.execute(
                "insert into library_fts(rowid, title, chapter, body, note, tags) values (?, ?, ?, ?, ?, ?)",
                params![id, searchable(&document.book_title), searchable(&document.chapter), searchable(&document.body), document.note.as_deref().map(searchable), searchable(&tags)],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let query = searchable(query);
        if query.is_empty() {
            return Ok(vec![]);
        }
        let mut statement = self.connection.prepare(
            "
select d.book_id, d.book_title, d.source_path, d.chapter, d.inner_book, d.chapter_index, d.line, d.body, d.note, d.tags,
       bm25(library_fts, 5.0, 3.0, 1.0, 4.0, 4.0)
from library_fts
join library_documents d on d.document_id = library_fts.rowid
where library_fts match ?
order by 8
limit ?
",
        )?;
        let hits = statement.query_map(params![query, limit as i64], |row| {
            let bytes: Vec<u8> = row.get(0)?;
            let book_id =
                BookId::from_bytes(bytes.as_slice()).ok_or(rusqlite::Error::InvalidQuery)?;
            let source_path = row.get::<_, String>(2)?;
            Ok(SearchHit {
                book_id,
                book_title: row.get(1)?,
                source_path: (!source_path.is_empty()).then_some(source_path),
                chapter: row.get(3)?,
                inner_book: row.get::<_, i64>(4)? as usize,
                chapter_index: row.get::<_, i64>(5)? as usize,
                line: row.get::<_, i64>(6)? as usize,
                body: row.get(7)?,
                note: row.get(8)?,
                tags: row
                    .get::<_, String>(9)?
                    .split_whitespace()
                    .map(ToOwned::to_owned)
                    .collect(),
                score: row.get(10)?,
            })
        })?;
        Ok(hits.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let mut statement = connection.prepare(&format!("pragma table_info({table})"))?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == column);
    if !exists {
        connection.execute_batch(&format!(
            "alter table {table} add column {column} {definition}"
        ))?;
    }
    Ok(())
}

fn searchable(text: &str) -> String {
    let mut terms: Vec<String> = text
        .split_whitespace()
        .filter(|word| !word.chars().any(is_cjk))
        .map(ToOwned::to_owned)
        .collect();
    let cjk: Vec<char> = text.chars().filter(|ch| is_cjk(*ch)).collect();
    for pair in cjk.windows(2) {
        terms.push(pair.iter().collect());
    }
    terms.join(" ")
}

fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::{searchable, LibraryDocument, LibraryIndex};
    use crate::annotations::BookId;

    #[test]
    fn cjk_queries_receive_bigram_terms() {
        assert_eq!(searchable("进程调度"), "进程 程调 调度");
    }

    #[test]
    fn index_searches_cjk_body_and_notes() {
        let mut index =
            LibraryIndex::open(&std::env::temp_dir().join("tbr-plus-library-test.sqlite")).unwrap();
        let book_id = BookId::from_content(b"one", "OSTEP", None);
        index
            .replace_book(
                &book_id,
                &[LibraryDocument {
                    book_id: book_id.clone(),
                    book_title: "OSTEP".into(),
                    source_path: "C:/books/ostep.epub".into(),
                    chapter: "调度".into(),
                    inner_book: 0,
                    chapter_index: 2,
                    line: 7,
                    body: "进程调度决定下一个运行者。".into(),
                    note: Some("复习调度器".into()),
                    tags: vec!["操作系统".into()],
                }],
            )
            .unwrap();
        assert_eq!(index.search("进程调度", 10).unwrap().len(), 1);
        let hit = &index.search("调度器", 10).unwrap()[0];
        assert_eq!(hit.book_title, "OSTEP");
        assert_eq!(hit.source_path.as_deref(), Some("C:/books/ostep.epub"));
        let _ = std::fs::remove_file(std::env::temp_dir().join("tbr-plus-library-test.sqlite"));
    }
}
