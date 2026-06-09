#![allow(unknown_lints)]
#![allow(clippy::collapsible_if)]

//! fio-fetcher: Fetch FIO Banka transactions and store them locally or in Google Sheets.
//!
//! This crate provides:
//! - `client` — FIO REST API client (`FioClient` trait, `FioApiClient` impl, `Transaction`, `AccountInfo`)
//! - `fetcher` — Core fetch logic that orchestrates fetching and storage
//! - `storage` — Transaction storage trait and implementations (filesystem, gdrive)

pub mod client;
pub mod fetcher;
pub mod storage;

// Re-export key types for convenience
pub use client::api::FioApiClient;
pub use client::{AccountInfo, FioClient, Transaction};
pub use fetcher::{FetchResult, FetchStatus};
pub use storage::{FilesystemStorage, TransactionStorage};

#[cfg(feature = "gdrive")]
pub use storage::GDriveStorage;
