#![allow(unknown_lints)]
#![allow(clippy::collapsible_if)]

//! Filesystem-based transaction storage.
//!
//! Stores transactions as JSONL files organized by year:
//! ```text
//! {message_dir}/{account_id}/
//!   YYYY/
//!     YYYY-MM.jsonl      # One JSON line per transaction
//!   index.json            # Account metadata
//! ```

use anyhow::Result;
use chrono::NaiveDate;
use chrono::Utc;
use std::fs;
use std::path::PathBuf;

use super::{AccountInfo, StoreResult, TransactionStorage};
use crate::client::Transaction;

/// Filesystem-based transaction storage.
///
/// Stores transactions as JSON lines in year/month-organized files.
pub struct FilesystemStorage {
    base_dir: PathBuf,
    account_id: String,
}

impl FilesystemStorage {
    pub fn new(base_dir: PathBuf, account_id: &str) -> Self {
        Self {
            base_dir,
            account_id: account_id.to_string(),
        }
    }

    /// Returns the account directory.
    fn account_dir(&self) -> PathBuf {
        self.base_dir.join(&self.account_id)
    }

    /// Load the set of transaction IDs from all JSONL files.
    #[allow(dead_code)]
    fn load_existing_ids(&self) -> std::collections::HashSet<i64> {
        let mut ids = std::collections::HashSet::new();
        let account_dir = self.account_dir();

        // Walk year directories
        if let Ok(year_entries) = fs::read_dir(&account_dir) {
            for year_entry in year_entries.flatten() {
                let year_path = year_entry.path();
                if !year_path.is_dir() {
                    continue;
                }
                // Walk JSONL files in year directory
                if let Ok(month_entries) = fs::read_dir(&year_path) {
                    for month_entry in month_entries.flatten() {
                        let month_path = month_entry.path();
                        if month_path.extension().is_some_and(|e| e == "jsonl") {
                            if let Ok(contents) = fs::read_to_string(&month_path) {
                                for line in contents.lines() {
                                    if let Ok(txn) = serde_json::from_str::<serde_json::Value>(line)
                                    {
                                        if let Some(id) = txn.get("id").and_then(|v| v.as_i64()) {
                                            ids.insert(id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        ids
    }
}

impl TransactionStorage for FilesystemStorage {
    fn transaction_exists(&self, transaction_id: i64) -> bool {
        // Quick check: scan JSONL files for matching ID
        // For better performance with large datasets, we could cache this
        let account_dir = self.account_dir();
        if !account_dir.exists() {
            return false;
        }

        // Walk year directories
        if let Ok(year_entries) = fs::read_dir(&account_dir) {
            for year_entry in year_entries.flatten() {
                let year_path = year_entry.path();
                if !year_path.is_dir() {
                    continue;
                }
                if let Ok(month_entries) = fs::read_dir(&year_path) {
                    for month_entry in month_entries.flatten() {
                        let month_path = month_entry.path();
                        if month_path.extension().is_some_and(|e| e == "jsonl") {
                            if let Ok(contents) = fs::read_to_string(&month_path) {
                                for line in contents.lines() {
                                    // Fast check: does the line contain the ID?
                                    if line.contains(&transaction_id.to_string()) {
                                        // Confirmed match via JSON parse
                                        if let Ok(txn) =
                                            serde_json::from_str::<serde_json::Value>(line)
                                        {
                                            if txn.get("id").and_then(|v| v.as_i64())
                                                == Some(transaction_id)
                                            {
                                                return true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    fn get_latest_transaction_date(&self) -> Result<Option<NaiveDate>> {
        let account_dir = self.account_dir();
        if !account_dir.exists() {
            return Ok(None);
        }

        let mut latest: Option<NaiveDate> = None;

        if let Ok(year_entries) = fs::read_dir(&account_dir) {
            for year_entry in year_entries.flatten() {
                let year_path = year_entry.path();
                if !year_path.is_dir() {
                    continue;
                }
                if let Ok(month_entries) = fs::read_dir(&year_path) {
                    for month_entry in month_entries.flatten() {
                        let month_path = month_entry.path();
                        if month_path.extension().is_some_and(|e| e == "jsonl") {
                            if let Ok(contents) = fs::read_to_string(&month_path) {
                                for line in contents.lines() {
                                    if let Ok(txn) = serde_json::from_str::<serde_json::Value>(line)
                                    {
                                        if let Some(date_str) =
                                            txn.get("date").and_then(|v| v.as_str())
                                        {
                                            if let Ok(date) =
                                                NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                                            {
                                                latest = Some(
                                                    latest.map_or(date, |l: NaiveDate| l.max(date)),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(latest)
    }

    fn store_transaction(
        &mut self,
        txn: &Transaction,
        account_info: &AccountInfo,
    ) -> Result<StoreResult> {
        let year = txn.date.format("%Y").to_string();
        let year_month = txn.date.format("%Y-%m").to_string();

        let account_dir = self.account_dir();
        let year_dir = account_dir.join(&year);
        fs::create_dir_all(&year_dir)?;

        let jsonl_path = year_dir.join(format!("{}.jsonl", year_month));

        // Append JSON line to the JSONL file
        let json_line = serde_json::to_string(txn)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&jsonl_path)?;
        use std::io::Write;
        writeln!(file, "{}", json_line)?;

        log::info!("Stored transaction {} to {}", txn.id, jsonl_path.display());

        // Update index.json (create or update)
        let index_path = account_dir.join("index.json");
        let index = if index_path.exists() {
            let content = fs::read_to_string(&index_path)?;
            serde_json::from_str(&content)?
        } else {
            serde_json::json!({})
        };

        // Merge account info into index
        let mut index_obj = index;
        index_obj["account_id"] = serde_json::Value::String(account_info.account_id.clone());
        index_obj["currency"] = serde_json::Value::String(account_info.currency.clone());
        index_obj["iban"] = serde_json::Value::String(account_info.iban.clone());
        index_obj["bic"] = serde_json::Value::String(account_info.bic.clone());
        index_obj["opening_balance"] = serde_json::Value::Number(
            serde_json::Number::from_f64(account_info.opening_balance)
                .unwrap_or_else(|| serde_json::Number::from(0)),
        );
        index_obj["closing_balance"] = serde_json::Value::Number(
            serde_json::Number::from_f64(account_info.closing_balance)
                .unwrap_or_else(|| serde_json::Number::from(0)),
        );
        index_obj["last_sync"] =
            serde_json::Value::String(Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string());

        let index_json = serde_json::to_string_pretty(&index_obj)?;
        fs::write(&index_path, index_json)?;

        Ok(StoreResult {
            path: jsonl_path,
            description: format!("{}/{}", year, year_month),
        })
    }

    fn storage_info(&self) -> Option<String> {
        Some(format!(
            "Transactions stored in: {}",
            self.account_dir().display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::AccountInfo;

    fn make_test_txn(id: i64, date: &str, amount: f64) -> Transaction {
        Transaction {
            id,
            date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
            amount,
            currency: "CZK".to_string(),
            counter_account: "123456789".to_string(),
            counter_account_name: "Test".to_string(),
            bank_code: "2010".to_string(),
            vs: "".to_string(),
            ks: "".to_string(),
            ss: "".to_string(),
            user_id: "".to_string(),
            transaction_type: "Platba".to_string(),
            performed: "".to_string(),
            message: "".to_string(),
            comment: "".to_string(),
            instruction_id: 0,
            bic: "".to_string(),
        }
    }

    fn make_test_info() -> AccountInfo {
        AccountInfo {
            account_id: "2301234567".to_string(),
            currency: "CZK".to_string(),
            iban: "CZ1234567890123456789012".to_string(),
            bic: "FIOBCZPP".to_string(),
            opening_balance: 100000.0,
            closing_balance: 95000.0,
        }
    }

    #[test]
    fn test_filesystem_storage_roundtrip() {
        let dir = std::env::temp_dir().join("fio_test_roundtrip");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut storage = FilesystemStorage::new(dir.clone(), "2301234567");
        let txn = make_test_txn(12345, "2024-01-15", -1500.0);
        let info = make_test_info();

        assert!(!storage.transaction_exists(12345));

        storage.store_transaction(&txn, &info).unwrap();

        assert!(storage.transaction_exists(12345));
        assert!(!storage.transaction_exists(99999));

        let latest = storage.get_latest_transaction_date().unwrap();
        assert_eq!(latest, Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_get_latest_date_empty_dir() {
        let dir = std::env::temp_dir().join("fio_test_empty");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let storage = FilesystemStorage::new(dir.clone(), "9999999999");
        let result = storage.get_latest_transaction_date().unwrap();
        assert!(result.is_none());

        let _ = fs::remove_dir_all(&dir);
    }
}
