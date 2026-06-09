//! FIO Banka REST API client implementation.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use log::{debug, info, warn};
use serde::Deserialize;
use std::time::Instant;

use super::{AccountInfo, FioClient, Transaction};

const BASE_URL: &str = "https://fioapi.fio.cz/v1/rest";

/// FIO Banka API client using reqwest blocking HTTP client.
///
/// Handles JSON deserialization of FIO's column-based format
/// and enforces minimum 30-second spacing between API calls per token.
pub struct FioApiClient {
    token: String,
    client: reqwest::blocking::Client,
    last_request: std::sync::Mutex<Option<Instant>>,
}

impl FioApiClient {
    pub fn new(token: &str) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
        Self {
            token: token.to_string(),
            client,
            last_request: std::sync::Mutex::new(None),
        }
    }

    /// Ensure at least 30 seconds have passed since the last API request.
    /// FIO enforces a rate limit of 1 request per 30 seconds per token.
    fn rate_limit(&self) {
        let mut guard = self.last_request.lock().unwrap();
        if let Some(last) = *guard {
            let elapsed = last.elapsed();
            if elapsed < std::time::Duration::from_secs(30) {
                let wait = std::time::Duration::from_secs(30) - elapsed;
                warn!("Rate limit: waiting {:?} before next FIO API call", wait);
                std::thread::sleep(wait);
            }
        }
        *guard = Some(Instant::now());
    }

    /// Fetch the full FIO API response for a date range.
    fn fetch_period(&self, from: &NaiveDate, to: &NaiveDate) -> Result<FioApiResponse> {
        self.rate_limit();
        let url = format!(
            "{}/periods/{}/{}/{}/transactions.json",
            BASE_URL,
            self.token,
            from.format("%Y-%m-%d"),
            to.format("%Y-%m-%d"),
        );
        info!("Fetching transactions from {} to {}", from, to);
        debug!(
            "FIO API URL: {} (token hidden)",
            &url[..url.len().saturating_sub(self.token.len() + 8)]
        );

        let response = self
            .client
            .get(&url)
            .send()
            .with_context(|| "Failed to fetch transactions from FIO API".to_string())?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            anyhow::bail!(
                "FIO API returned HTTP {}: {}",
                status,
                body.chars().take(500).collect::<String>()
            );
        }

        let body = response
            .text()
            .with_context(|| "Failed to read FIO API response")?;
        let api_response: FioApiResponse =
            serde_json::from_str(&body).with_context(|| "Failed to parse FIO API response")?;

        Ok(api_response)
    }
}

impl FioClient for FioApiClient {
    fn get_transactions(&self, from: &NaiveDate, to: &NaiveDate) -> Result<Vec<Transaction>> {
        let api_response = self.fetch_period(from, to)?;
        let transactions = api_response
            .account_statement
            .transaction_list
            .map(|tl| {
                tl.transaction
                    .into_iter()
                    .filter_map(|raw| raw.to_transaction())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        info!(
            "Fetched {} transactions for period {} to {}",
            transactions.len(),
            from,
            to
        );
        Ok(transactions)
    }

    fn get_info(&self, from: &NaiveDate, to: &NaiveDate) -> Result<AccountInfo> {
        let api_response = self.fetch_period(from, to)?;
        let info = api_response.account_statement.info;
        Ok(AccountInfo {
            account_id: info.account_id,
            currency: info.currency,
            iban: info.iban,
            bic: info.bic,
            opening_balance: info.opening_balance,
            closing_balance: info.closing_balance,
        })
    }
}

// ── FIO API JSON response types ────────────────────────────────────

/// Top-level FIO API response.
#[derive(Debug, Deserialize)]
struct FioApiResponse {
    account_statement: FioAccountStatement,
}

#[derive(Debug, Deserialize)]
struct FioAccountStatement {
    info: FioAccountInfo,
    transaction_list: Option<FioTransactionList>,
}

#[derive(Debug, Deserialize)]
struct FioAccountInfo {
    #[serde(rename = "accountId")]
    account_id: String,
    currency: String,
    iban: String,
    bic: String,
    #[serde(rename = "openingBalance")]
    opening_balance: f64,
    #[serde(rename = "closingBalance")]
    closing_balance: f64,
}

#[derive(Debug, Deserialize)]
struct FioTransactionList {
    transaction: Vec<FioRawTransaction>,
}

/// A raw FIO transaction with column-based values.
///
/// FIO returns each transaction field as `columnN: {name: "...", value: "..."}`.
/// We deserialize this into a HashMap-like structure for robust extraction.
#[derive(Debug, Deserialize)]
struct FioRawTransaction {
    /// Dynamic columns keyed as "column0", "column1", etc.
    #[serde(flatten)]
    columns: std::collections::HashMap<String, FioColumnValue>,
}

#[derive(Debug, Deserialize)]
struct FioColumnValue {
    #[allow(dead_code)]
    name: Option<String>,
    value: serde_json::Value,
}

impl FioRawTransaction {
    /// Convert a raw FIO transaction into a typed Transaction struct.
    fn to_transaction(&self) -> Option<Transaction> {
        let id = self.get_i64("column22")?;
        let date = self.get_date("column0")?;
        let amount = self.get_f64("column1").unwrap_or(0.0);

        Some(Transaction {
            id,
            date,
            amount,
            currency: self.get_string("column2").unwrap_or_default(),
            counter_account: self.get_string("column3").unwrap_or_default(),
            counter_account_name: self.get_string("column14").unwrap_or_default(),
            bank_code: self.get_string("column4").unwrap_or_default(),
            vs: self.get_string("column5").unwrap_or_default(),
            ks: self.get_string("column6").unwrap_or_default(),
            ss: self.get_string("column7").unwrap_or_default(),
            user_id: self.get_string("column8").unwrap_or_default(),
            transaction_type: self.get_string("column9").unwrap_or_default(),
            performed: self.get_string("column10").unwrap_or_default(),
            message: self.get_string("column16").unwrap_or_default(),
            comment: self.get_string("column25").unwrap_or_default(),
            instruction_id: self.get_i64("column18").unwrap_or(0),
            bic: self.get_string("column26").unwrap_or_default(),
        })
    }

    fn get_string(&self, key: &str) -> Option<String> {
        self.columns
            .get(key)
            .and_then(|c| c.value.as_str())
            .map(|s| s.to_string())
    }

    fn get_i64(&self, key: &str) -> Option<i64> {
        self.columns
            .get(key)
            .and_then(|c| c.value.as_i64())
            .or_else(|| {
                // FIO sometimes returns numeric IDs as strings
                self.columns
                    .get(key)
                    .and_then(|c| c.value.as_str())
                    .and_then(|s| s.parse().ok())
            })
    }

    fn get_f64(&self, key: &str) -> Option<f64> {
        self.columns
            .get(key)
            .and_then(|c| c.value.as_f64())
            .or_else(|| {
                self.columns
                    .get(key)
                    .and_then(|c| c.value.as_str())
                    .and_then(|s| s.parse().ok())
            })
    }

    /// Parse a FIO date string. FIO uses format "2024-01-15+0200" (timezone offset without colon)
    /// or "2024-01-15" (plain date).
    fn get_date(&self, key: &str) -> Option<NaiveDate> {
        let s = self.get_string(key)?;
        // Strip timezone offset: "2024-01-15+0200" → "2024-01-15"
        let date_str = if let Some(pos) = s.find('+') {
            &s[..pos]
        } else if let Some(pos) = s.find('T') {
            &s[..pos]
        } else {
            &s
        };
        NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_transaction_to_transaction() {
        let raw_json = r#"{
            "column22": {"name": "ID pohybu", "value": 12345678},
            "column0": {"name": "Datum", "value": "2024-01-15+0200"},
            "column1": {"name": "Objem", "value": -1500.0},
            "column2": {"name": "Měna", "value": "CZK"},
            "column3": {"name": "Protiúčet", "value": "123456789"},
            "column4": {"name": "Kód banky", "value": "2010"},
            "column5": {"name": "VS", "value": "12345678"},
            "column6": {"name": "KS", "value": ""},
            "column7": {"name": "SS", "value": ""},
            "column8": {"name": "Uživatelská identifikace", "value": ""},
            "column9": {"name": "Typ", "value": "Platba"},
            "column10": {"name": "Provedl", "value": ""},
            "column14": {"name": "Název protiúčtu", "value": "John Doe"},
            "column16": {"name": "Zpráva pro příjemce", "value": "Invoice 123"},
            "column18": {"name": "ID pokynu", "value": 12345678},
            "column25": {"name": "Komentář", "value": ""},
            "column26": {"name": "BIC", "value": ""}
        }"#;

        let raw: FioRawTransaction = serde_json::from_str(raw_json).unwrap();
        let txn = raw.to_transaction().unwrap();

        assert_eq!(txn.id, 12345678);
        assert_eq!(txn.date, NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
        assert_eq!(txn.amount, -1500.0);
        assert_eq!(txn.currency, "CZK");
        assert_eq!(txn.counter_account, "123456789");
        assert_eq!(txn.counter_account_name, "John Doe");
        assert_eq!(txn.bank_code, "2010");
        assert_eq!(txn.vs, "12345678");
        assert_eq!(txn.transaction_type, "Platba");
        assert_eq!(txn.message, "Invoice 123");
    }

    #[test]
    fn test_date_parsing_with_timezone() {
        let raw_json = r#"{
            "column22": {"name": "ID", "value": 1},
            "column0": {"name": "Datum", "value": "2024-06-30+0200"},
            "column1": {"name": "Objem", "value": 100.0},
            "column2": {"name": "Měna", "value": "CZK"}
        }"#;

        let raw: FioRawTransaction = serde_json::from_str(raw_json).unwrap();
        let txn = raw.to_transaction().unwrap();
        assert_eq!(txn.date, NaiveDate::from_ymd_opt(2024, 6, 30).unwrap());
    }

    #[test]
    fn test_date_parsing_plain() {
        let raw_json = r#"{
            "column22": {"name": "ID", "value": 1},
            "column0": {"name": "Datum", "value": "2024-01-15"},
            "column1": {"name": "Objem", "value": 100.0},
            "column2": {"name": "Měna", "value": "CZK"}
        }"#;

        let raw: FioRawTransaction = serde_json::from_str(raw_json).unwrap();
        let txn = raw.to_transaction().unwrap();
        assert_eq!(txn.date, NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
    }
}
