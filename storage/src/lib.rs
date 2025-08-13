//! Storage engine for JDB database
//!
//! This crate provides the low-level storage primitives including
//! pages, B-trees, and buffer management.

pub mod page;

pub use page::{Page, PageHeader, PageType, SlotEntry};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Page {0} not found")]
    PageNotFound(u32),

    #[error("Page {0} is full")]
    PageFull(u32),

    #[error("Invalid slot index {index} for page {page_id}")]
    InvalidSlot { page_id: u32, index: usize },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Checksum mismatch for page {0}")]
    ChecksumMismatch(u32),
}

pub type Result<T> = std::result::Result<T, StorageError>;
