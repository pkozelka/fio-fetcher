use anyhow::Result;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Trait for FIO Banka API client.
pub trait FioClient {
    /// Fetch transactions for a date range.
    fn get_transactions(&self, from: &NaiveDate, to: &NaiveDate) -> Result<Vec<Transaction>>;

    /// Fetch account info (requires at least one API call).
    fn get_info(&self, from: &NaiveDate, to: &NaiveDate) -> Result<AccountInfo>;
}

/// A single FIO bank transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Movement ID (column22)
    pub id: i64,
    /// Transaction date (column0)
    #[serde(with = "naive_date_opt")]
    pub date: NaiveDate,
    /// Amount (column1, negative for debits)
    pub amount: f64,
    /// Currency (column2)
    pub currency: String,
    /// Counter account number (column3)
    #[serde(default)]
    pub counter_account: String,
    /// Counter account name (column14)
    #[serde(default)]
    pub counter_account_name: String,
    /// Bank code of counter account (column4)
    #[serde(default)]
    pub bank_code: String,
    /// Variable symbol (column5)
    #[serde(default)]
    pub vs: String,
    /// Constant symbol (column6)
    #[serde(default)]
    pub ks: String,
    /// Specific symbol (column7)
    #[serde(default)]
    pub ss: String,
    /// User identification (column8)
    #[serde(default)]
    pub user_id: String,
    /// Transaction type (column9)
    #[serde(default)]
    pub transaction_type: String,
    /// Performed by (column10)
    #[serde(default)]
    pub performed: String,
    /// Message for recipient (column16)
    #[serde(default)]
    pub message: String,
    /// Comment (column25)
    #[serde(default)]
    pub comment: String,
    /// Instruction ID (column18)
    #[serde(default)]
    pub instruction_id: i64,
    /// BIC/SWIFT code (column26)
    #[serde(default)]
    pub bic: String,
}

/// Account info from the FIO API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub account_id: String,
    pub currency: String,
    pub iban: String,
    pub bic: String,
    pub opening_balance: f64,
    pub closing_balance: f64,
}

/// Custom serde helper for NaiveDate that handles missing/empty dates.
mod naive_date_opt {
    use chrono::NaiveDate;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(date: &NaiveDate, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&date.format("%Y-%m-%d").to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveDate, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        NaiveDate::parse_from_str(&s, "%Y-%m-%d").map_err(serde::de::Error::custom)
    }
}

pub mod api;
