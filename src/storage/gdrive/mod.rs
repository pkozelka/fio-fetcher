//! Google Sheets storage backend for FIO banka transactions.
//!
//! This module implements the [`TransactionStorage`] trait using Google Sheets
//! for transaction indexing and Google Drive for file storage.
//!
//! # Architecture
//!
//! The module is split into focused submodules:
//!
//! - **types** — API response types (service account credentials, JWT claims,
//!   Drive/Sheets REST types)
//! - **auth** — OAuth2 access token acquisition via JWT grant flow
//! - **drive** — Google Drive file/folder operations (find, create, upload)
//! - **sheets** — Google Sheets operations (create tabs, format, append rows,
//!   query for transaction ID, find latest date)
//! - **storage_impl** — [`TransactionStorage`] trait implementation
//! - **util** — MIME type lookup and unit tests

mod auth;
mod drive;
mod sheets;
mod storage_impl;
pub mod types;
mod util;

use anyhow::{Context, Result};
use log::info;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use types::ServiceAccountCredentials;

/// A row buffered for batch append to a Sheets tab.
struct PendingRow {
    sheet_name: String,
    values: Vec<String>,
    /// Transaction ID extracted from values.
    /// Used to match updates that arrive before flush.
    #[allow(dead_code)]
    transaction_id: i64,
}

// ---------------------------------------------------------------------------
// Google API URLs
// ---------------------------------------------------------------------------

const DRIVE_API_URL: &str = "https://www.googleapis.com/drive/v3";
const SHEETS_API_URL: &str = "https://sheets.googleapis.com/v4/spreadsheets";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPES: &str =
    "https://www.googleapis.com/auth/drive https://www.googleapis.com/auth/spreadsheets";

// ---------------------------------------------------------------------------
// GDriveStorage struct
// ---------------------------------------------------------------------------

/// Google Drive + Sheets storage backend for FIO transactions.
///
/// Uses a Google Workspace service account to:
/// - Create/resolve a folder hierarchy on Drive
/// - Maintain a shared spreadsheet index
/// - Record transaction metadata in year-based sheet tabs
pub struct GDriveStorage {
    #[allow(dead_code)]
    credentials_path: String,
    credentials: ServiceAccountCredentials,
    root_folder_name: String,
    spreadsheet_name: String,
    /// FIO account ID (e.g. "2301234567")
    account_id: String,
    /// Account name for display in Overview
    account_name: Option<String>,
    /// Account currency for display in Overview
    account_currency: Option<String>,
    impersonate_user: Option<String>,

    // Shared HTTP client — reused across all API calls for connection pooling
    http_client: OnceLock<reqwest::blocking::Client>,

    // Cached IDs (lazily resolved on first access)
    root_folder_id: Mutex<Option<String>>,
    spreadsheet_id: Mutex<Option<String>>,
    spreadsheet_parent_id: Mutex<Option<String>>,

    // Folder ID cache
    folder_cache: Mutex<HashMap<String, String>>,

    // Buffered sheet rows, flushed in batch
    pending_rows: Arc<Mutex<Vec<PendingRow>>>,

    // OAuth2 token cache
    access_token: Mutex<Option<String>>,
    token_expires_at: Mutex<u64>,
}

impl GDriveStorage {
    /// Create a new GDriveStorage from a service account credentials JSON file.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        credentials_path: &Path,
        root_folder_name: &str,
        spreadsheet_name: &str,
        account_id: &str,
        account_name: Option<&str>,
        account_currency: Option<&str>,
        impersonate_user: Option<&str>,
    ) -> Result<Self> {
        let json = std::fs::read_to_string(credentials_path)
            .with_context(|| format!("Failed to read credentials from {:?}", credentials_path))?;
        let credentials: ServiceAccountCredentials = serde_json::from_str(&json)
            .with_context(|| "Failed to parse service account credentials JSON")?;

        info!(
            "Loaded Google service account credentials for {}",
            credentials.client_email
        );

        Ok(Self {
            credentials_path: credentials_path.to_string_lossy().to_string(),
            credentials,
            root_folder_name: root_folder_name.to_string(),
            spreadsheet_name: spreadsheet_name.to_string(),
            account_id: account_id.to_string(),
            account_name: account_name.map(|s| s.to_string()),
            account_currency: account_currency.map(|s| s.to_string()),
            impersonate_user: impersonate_user.map(|s| s.to_string()),
            http_client: OnceLock::new(),
            root_folder_id: Mutex::new(None),
            spreadsheet_id: Mutex::new(None),
            spreadsheet_parent_id: Mutex::new(None),
            folder_cache: Mutex::new(HashMap::new()),
            pending_rows: Arc::new(Mutex::new(Vec::new())),
            access_token: Mutex::new(None),
            token_expires_at: Mutex::new(0),
        })
    }

    /// Returns a shared HTTP client for connection pooling.
    fn http_client(&self) -> &reqwest::blocking::Client {
        self.http_client.get_or_init(reqwest::blocking::Client::new)
    }

    // ── Row buffering ──────────────────────────────────────────────

    /// Buffer a row for later batch-append to a Sheets tab.
    fn buffer_sheet_row(&self, sheet_name: &str, transaction_id: i64, values: Vec<String>) {
        self.pending_rows.lock().unwrap().push(PendingRow {
            sheet_name: sheet_name.to_string(),
            values,
            transaction_id,
        });
    }

    /// Flush all buffered rows to Google Sheets in batch.
    fn flush_pending_rows(&self, spreadsheet_id: &str) -> Result<()> {
        let rows = {
            let mut guard = self.pending_rows.lock().unwrap();
            std::mem::take(&mut *guard)
        };

        if rows.is_empty() {
            return Ok(());
        }

        // Group by sheet_name to batch rows per tab
        let mut groups: std::collections::BTreeMap<String, Vec<Vec<String>>> =
            std::collections::BTreeMap::new();

        for row in &rows {
            groups
                .entry(row.sheet_name.clone())
                .or_default()
                .push(row.values.clone());
        }

        for (sheet_name, batch) in &groups {
            self.batch_append_rows(spreadsheet_id, sheet_name, batch)?;
        }

        Ok(())
    }

    // ── Folder ID cache ─────────────────────────────────────────────

    fn cache_key(parent_id: &str, name: &str) -> String {
        format!("{}/{}", parent_id, name)
    }

    fn cache_key_root(name: &str) -> String {
        format!("ROOT/{}", name)
    }

    fn folder_cache_get(&self, key: &str) -> Option<String> {
        self.folder_cache.lock().unwrap().get(key).cloned()
    }

    fn folder_cache_put(&self, key: String, id: String) {
        self.folder_cache.lock().unwrap().insert(key, id);
    }
}
