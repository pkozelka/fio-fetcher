//! TransactionStorage implementation for Google Sheets.
//!
//! Adapts the datovka-fetcher pattern for FIO banka transactions:
//! - Overview tab: account info header, balance, last sync
//! - Year tabs: columns A-I = Date, Amount, Currency, Counter Account,
//!   Counter Account Name, VS, KS, SS, Comment

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};
use log::info;

use super::GDriveStorage;
use crate::client::{AccountInfo, Transaction};
use crate::storage::{StoreResult, TransactionStorage};

impl TransactionStorage for GDriveStorage {
    fn transaction_exists(&self, transaction_id: i64) -> bool {
        // Try to find the transaction in the year sheet(s)
        // We'll check the current year and previous year as a heuristic
        let spreadsheet_id = match self.ensure_spreadsheet() {
            Ok(id) => id,
            Err(e) => {
                log::warn!("Failed to ensure spreadsheet for existence check: {}", e);
                return false;
            }
        };

        let current_year = chrono::Local::now().format("%Y").to_string();
        let last_year = (chrono::Local::now().year() - 1).to_string();

        for sheet_name in [&current_year, &last_year] {
            match self.query_sheet_for_transaction_id(&spreadsheet_id, sheet_name, transaction_id) {
                Ok(true) => return true,
                Ok(false) => continue,
                Err(e) => {
                    log::debug!(
                        "Error checking sheet '{}' for transaction {}: {}",
                        sheet_name,
                        transaction_id,
                        e
                    );
                    continue;
                }
            }
        }

        false
    }

    fn get_latest_transaction_date(&self) -> Result<Option<NaiveDate>> {
        let spreadsheet_id = self.ensure_spreadsheet()?;

        // Check current year and previous year sheets
        let current_year = chrono::Local::now().format("%Y").to_string();
        let last_year = (chrono::Local::now().year() - 1).to_string();

        let mut latest: Option<NaiveDate> = None;

        for sheet_name in [&current_year, &last_year] {
            match self.get_latest_date_from_sheet(&spreadsheet_id, sheet_name) {
                Ok(Some(date)) => {
                    latest = Some(latest.map_or(date, |l: NaiveDate| l.max(date)));
                }
                Ok(None) => continue,
                Err(e) => {
                    log::debug!(
                        "Error getting latest date from sheet '{}': {}",
                        sheet_name,
                        e
                    );
                    continue;
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
        let spreadsheet_id = self.ensure_spreadsheet()?;
        let year = txn.date.format("%Y").to_string();
        let date_str = txn.date.format("%Y-%m-%d").to_string();

        // Build the row values for the year sheet
        // Columns: A=Date, B=Amount, C=Currency, D=Counter Account, E=Counter Account Name,
        //          F=VS, G=KS, H=SS, I=Comment
        let values = vec![
            date_str.clone(),
            format!("{}", txn.amount),
            txn.currency.clone(),
            if txn.counter_account.is_empty() {
                String::new()
            } else if !txn.bank_code.is_empty() {
                format!("{}/{}", txn.counter_account, txn.bank_code)
            } else {
                txn.counter_account.clone()
            },
            txn.counter_account_name.clone(),
            txn.vs.clone(),
            txn.ks.clone(),
            txn.ss.clone(),
            if txn.comment.is_empty() {
                txn.message.clone()
            } else if txn.message.is_empty() {
                txn.comment.clone()
            } else {
                format!("{} | {}", txn.comment, txn.message)
            },
        ];

        info!(
            "Storing transaction {} ({}) in sheet '{}'",
            txn.id, date_str, year
        );

        self.buffer_sheet_row(&year, txn.id, values);

        // Update the latest date in the Overview tab
        if let Err(e) = self.update_overview_latest_date(
            &spreadsheet_id,
            &year,
            &chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        ) {
            log::warn!("Failed to update latest date in Overview: {}", e);
        }

        // Update the account info in the Overview tab (row 2)
        if let Err(e) = self.update_overview_account_info(&spreadsheet_id, account_info) {
            log::warn!("Failed to update account info in Overview: {}", e);
        }

        Ok(StoreResult {
            path: std::path::PathBuf::from(format!(
                "gdrive://spreadsheets/{}/sheets/{}",
                spreadsheet_id, year
            )),
            description: format!("Sheet '{}' in spreadsheet {}", year, spreadsheet_id),
        })
    }

    fn flush(&mut self) -> Result<()> {
        let spreadsheet_id = self.ensure_spreadsheet()?;

        // Flush buffered rows to Sheets
        self.flush_pending_rows(&spreadsheet_id)?;

        // Update the last sync date in Overview
        let now = chrono::Local::now().format("%Y-%m-%d").to_string();
        if let Err(e) = self.update_last_sync_date(&spreadsheet_id, &now) {
            log::warn!("Failed to update last sync date: {}", e);
        }

        Ok(())
    }

    fn storage_info(&self) -> Option<String> {
        match self.ensure_spreadsheet() {
            Ok(id) => Some(format!(
                "Transactions stored in Google Sheets (spreadsheet id: {})",
                id
            )),
            Err(e) => Some(format!("Google Sheets storage (error: {})", e)),
        }
    }
}

impl GDriveStorage {
    /// Update the account info in the Overview tab (row 2).
    fn update_overview_account_info(
        &self,
        spreadsheet_id: &str,
        account_info: &AccountInfo,
    ) -> Result<()> {
        let token = self.get_access_token()?;
        let range = "Overview!A2:D2";
        let url = format!(
            "{}/{}/values/{}?valueInputOption=USER_ENTERED",
            SHEETS_API_URL,
            spreadsheet_id,
            urlencoding::encode(range)
        );

        let account_label = format!("Account: {}", account_info.account_id);
        let currency_label = format!("Currency: {}", account_info.currency);
        let iban_label = format!("IBAN: {}", account_info.iban);
        let bic_label = format!("BIC: {}", account_info.bic);

        let values = vec![vec![account_label, currency_label, iban_label, bic_label]];

        let body = serde_json::json!({ "values": values });

        let client = self.http_client();
        let resp = client
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .with_context(|| "Failed to update account info in Overview tab")?;

        if resp.status().is_success() {
            log::debug!("Updated account info in Overview tab");
        } else {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            log::warn!("Failed to update account info: {} {}", status, body);
        }

        Ok(())
    }
}

use super::SHEETS_API_URL;
