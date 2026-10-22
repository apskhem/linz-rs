//! Binary payloads (multipart files and `application/octet-stream` bodies) cannot be
//! represented in a [`serde_json::Value`], which is the intermediate format used for
//! validation. Instead, files are kept in a [`FileStore`] and referenced from the JSON value
//! by a placeholder string. [`crate::endpoint::File`] (de)serializes through the store that is
//! active on the current thread.

use std::cell::RefCell;

use crate::endpoint::File;
use crate::internal::utils;

const PLACEHOLDER_PREFIX: &str = "\u{0}linz-file:";

#[derive(Debug, Clone)]
pub(crate) struct FileStore {
    nonce: String,
    files: Vec<File>,
}

impl Default for FileStore {
    fn default() -> Self {
        Self {
            nonce: utils::random_hex(8),
            files: Vec::new(),
        }
    }
}

impl FileStore {
    /// Stores a file and returns its placeholder.
    pub fn insert(&mut self, file: File) -> String {
        self.files.push(file);
        format!("{PLACEHOLDER_PREFIX}{}:{}", self.nonce, self.files.len() - 1)
    }

    pub fn get(&self, placeholder: &str) -> Option<&File> {
        let rest = placeholder.strip_prefix(PLACEHOLDER_PREFIX)?;
        let (nonce, index) = rest.split_once(':')?;

        if nonce != self.nonce {
            return None;
        }

        self.files.get(index.parse::<usize>().ok()?)
    }
}

thread_local! {
    static INBOX: RefCell<Option<FileStore>> = const { RefCell::new(None) };
    static OUTBOX: RefCell<Option<FileStore>> = const { RefCell::new(None) };
}

/// Runs `f` with `store` available to [`File`] deserialization.
pub(crate) fn with_inbox<R>(store: &FileStore, f: impl FnOnce() -> R) -> R {
    let prev = INBOX.with(|cell| cell.replace(Some(store.clone())));
    let result = f();
    INBOX.with(|cell| cell.replace(prev));
    result
}

/// Runs `f` while collecting every [`File`] that is serialized.
pub(crate) fn capture<R>(f: impl FnOnce() -> R) -> (R, FileStore) {
    let prev = OUTBOX.with(|cell| cell.replace(Some(FileStore::default())));
    let result = f();
    let store = OUTBOX.with(|cell| cell.replace(prev)).unwrap_or_default();
    (result, store)
}

pub(crate) fn take_from_inbox(placeholder: &str) -> Option<File> {
    INBOX.with(|cell| cell.borrow().as_ref().and_then(|store| store.get(placeholder).cloned()))
}

pub(crate) fn put_into_outbox(file: &File) -> Option<String> {
    OUTBOX.with(|cell| cell.borrow_mut().as_mut().map(|store| store.insert(file.clone())))
}
