pub mod filesystem;

pub use filesystem::FilesystemStorage;

#[cfg(feature = "gdrive")]
pub mod gdrive;

#[cfg(feature = "gdrive")]
pub use gdrive::GDriveStorage;

use anyhow::Result;
use chrono::NaiveDate;

use crate::client::{AccountInfo, Transaction};

/// Result of storing a transaction.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StoreResult {
    /// Path or identifier of the stored transaction.
    pub path: std::path::PathBuf,
    /// Human-readable description.
    pub description: String,
}

/// Trait for transaction storage backends.
///
/// Implementations: filesystem (default), Google Sheets (feature-gated).
pub trait TransactionStorage: Send + Sync {
    /// Check if a transaction (by ID) already exists in storage.
    fn transaction_exists(&self, transaction_id: i64) -> bool;

    /// Get the latest transaction date from existing storage.
    /// Used for auto-resume (--from-date auto).
    fn get_latest_transaction_date(&self) -> Result<Option<NaiveDate>>;

    /// Store a transaction and return the storage result.
    fn store_transaction(
        &mut self,
        txn: &Transaction,
        account_info: &AccountInfo,
    ) -> Result<StoreResult>;

    /// Flush any buffered writes to the backend.
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    /// Return an optional info string about this storage backend.
    fn storage_info(&self) -> Option<String> {
        None
    }
}
