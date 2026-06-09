//! Core fetch logic for FIO Banka transactions.

use anyhow::Result;
use chrono::NaiveDate;

use crate::client::FioClient;
use crate::storage::TransactionStorage;

/// Result of fetching a single transaction.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FetchResult {
    pub id: i64,
    pub date: String,
    pub amount: f64,
    pub currency: String,
    pub counter_account_name: String,
    pub comment: String,
    pub status: FetchStatus,
}

/// Status of a transaction fetch attempt.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum FetchStatus {
    Stored,
    Skipped,
    Failed(String),
}

/// Fetch transactions from FIO Banka and store them.
///
/// `limit` is the max number of *new* (not-already-stored) transactions to process.
/// A limit of 0 means unlimited (process all new transactions).
pub fn fetch_transactions(
    client: &dyn FioClient,
    storage: &mut dyn TransactionStorage,
    from: &NaiveDate,
    to: &NaiveDate,
    limit: u32,
    json_output: bool,
) -> Result<Vec<FetchResult>> {
    // Get account info first
    let account_info = client.get_info(from, to)?;
    log::info!(
        "Account: {} ({}) — opening balance: {}, closing balance: {}",
        account_info.account_id,
        account_info.currency,
        account_info.opening_balance,
        account_info.closing_balance,
    );

    // Fetch transactions
    let transactions = client.get_transactions(from, to)?;
    log::info!(
        "Fetched {} transactions for period {} to {}",
        transactions.len(),
        from,
        to
    );

    let mut results = Vec::new();
    let mut stored_count: u32 = 0;

    for txn in &transactions {
        // If limit is set and we've stored enough, stop
        if limit > 0 && stored_count >= limit {
            log::info!("Reached storage limit of {} transactions, stopping", limit);
            break;
        }

        // Check if already stored
        if storage.transaction_exists(txn.id) {
            log::info!("Transaction {} already stored, skipping", txn.id);
            results.push(FetchResult {
                id: txn.id,
                date: txn.date.format("%Y-%m-%d").to_string(),
                amount: txn.amount,
                currency: txn.currency.clone(),
                counter_account_name: txn.counter_account_name.clone(),
                comment: txn.comment.clone(),
                status: FetchStatus::Skipped,
            });
            continue;
        }

        // Store the transaction
        match storage.store_transaction(txn, &account_info) {
            Ok(_store_result) => {
                log::info!(
                    "Stored transaction {} ({} {} {})",
                    txn.id,
                    txn.date.format("%Y-%m-%d"),
                    txn.amount,
                    txn.currency,
                );
                results.push(FetchResult {
                    id: txn.id,
                    date: txn.date.format("%Y-%m-%d").to_string(),
                    amount: txn.amount,
                    currency: txn.currency.clone(),
                    counter_account_name: txn.counter_account_name.clone(),
                    comment: txn.comment.clone(),
                    status: FetchStatus::Stored,
                });
                stored_count += 1;
            }
            Err(e) => {
                log::error!("Failed to store transaction {}: {}", txn.id, e);
                results.push(FetchResult {
                    id: txn.id,
                    date: txn.date.format("%Y-%m-%d").to_string(),
                    amount: txn.amount,
                    currency: txn.currency.clone(),
                    counter_account_name: txn.counter_account_name.clone(),
                    comment: txn.comment.clone(),
                    status: FetchStatus::Failed(e.to_string()),
                });
            }
        }
    }

    // Flush any buffered writes
    storage.flush()?;

    if json_output {
        let non_skipped: Vec<&FetchResult> = results
            .iter()
            .filter(|r| !matches!(r.status, FetchStatus::Skipped))
            .collect();
        println!("{}", serde_json::to_string_pretty(&non_skipped)?);
    } else {
        print_results(&results);
    }

    // Log storage info
    if let Some(info) = storage.storage_info() {
        log::info!("{}", info);
    }

    Ok(results)
}

/// Column widths for the results table.
const W_DATE: usize = 10;
const W_ID: usize = 10;
const W_AMOUNT: usize = 12;
const W_CURRENCY: usize = 3;
const W_PARTY: usize = 25;
const W_COMMENT: usize = 30;

fn trunc(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{truncated}…")
    }
}

fn print_results(results: &[FetchResult]) {
    let stored: Vec<&FetchResult> = results
        .iter()
        .filter(|r| matches!(r.status, FetchStatus::Stored))
        .collect();
    let skipped = results
        .iter()
        .filter(|r| matches!(r.status, FetchStatus::Skipped))
        .count();
    let failed: Vec<&FetchResult> = results
        .iter()
        .filter(|r| matches!(r.status, FetchStatus::Failed(_)))
        .collect();

    if !stored.is_empty() || !failed.is_empty() {
        println!("FIO Transaction Fetch Results");
        println!("=============================");
        println!();
        println!(
            "{:W_DATE$} {:W_ID$} {:W_AMOUNT$} {:W_CURRENCY$} {:W_PARTY$} COMMENT",
            "DATE", "ID", "AMOUNT", "CUR", "PARTY",
        );
        println!(
            "{:W_DATE$} {:W_ID$} {:W_AMOUNT$} {:W_CURRENCY$} {:W_PARTY$} {}",
            "----------",
            "----------",
            "------------",
            "---",
            "-------------------------",
            "-".repeat(W_COMMENT),
        );

        let all_rows: Vec<&FetchResult> = stored.iter().chain(failed.iter()).copied().collect();
        for r in &all_rows {
            let _status = match &r.status {
                FetchStatus::Stored => "OK",
                FetchStatus::Skipped => "SKIP",
                FetchStatus::Failed(_) => "FAIL",
            };
            let amount_str = format!("{}", r.amount);
            println!(
                "{:W_DATE$} {:W_ID$} {:W_AMOUNT$} {:W_CURRENCY$} {:W_PARTY$} {}",
                r.date,
                trunc(&r.id.to_string(), W_ID),
                trunc(&amount_str, W_AMOUNT),
                r.currency,
                trunc(&r.counter_account_name, W_PARTY),
                trunc(&r.comment, W_COMMENT - 6),
            );
            if let FetchStatus::Failed(err) = &r.status {
                println!(
                    "{:W_DATE$} {:W_ID$} {:W_AMOUNT$} {:W_CURRENCY$} {:W_PARTY$}   → Error: {err}",
                    "", "", "", "", ""
                );
            }
        }
    }

    println!();
    println!(
        "Total: {} transactions ({} stored, {} skipped, {} failed)",
        results.len(),
        stored.len(),
        skipped,
        failed.len(),
    );
}
