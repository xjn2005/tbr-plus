mod index;
mod indexer;

pub(crate) use index::{LibraryDocument, LibraryIndex, SearchHit};
pub(crate) use indexer::collect_file_documents;
