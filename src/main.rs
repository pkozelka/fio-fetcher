//! fio-fetcher: CLI tool for fetching FIO Banka transactions and storing them locally or in Google Sheets.

use anyhow::Result;
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod client;
mod fetcher;
mod storage;

use client::api::FioApiClient;
use storage::TransactionStorage;

/// CLI tool for fetching FIO Banka transactions.
#[derive(Parser)]
#[command(name = "fio-fetcher")]
#[command(
    version,
    about = "Fetch FIO Banka transactions via REST API and store them locally or in Google Sheets"
)]
struct Cli {
    /// Increase verbosity (-v for info, -vv for debug)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Fetch transactions for a specific account
    FetchAccount {
        /// Account ID to fetch (e.g. "2301234567")
        account_id: String,

        /// FIO API token (or read from config / FIO_TOKEN env var)
        #[arg(long, env = "FIO_TOKEN")]
        token: Option<String>,

        /// Start date (YYYY-MM-DD). Auto-resumes from last known date if omitted.
        #[arg(long, env = "FIO_FROM_DATE")]
        from_date: Option<String>,

        /// End date (YYYY-MM-DD). Defaults to today.
        #[arg(long, env = "FIO_TO_DATE")]
        to_date: Option<String>,

        /// Max transactions to process (0 = unlimited)
        #[arg(long, default_value = "0")]
        limit: u32,

        /// Storage backend: filesystem or gdrive
        #[arg(long, env = "FIO_STORAGE", default_value = "filesystem")]
        storage: String,

        /// Local storage directory for filesystem backend
        #[arg(long, env = "FIO_MESSAGE_DIR")]
        message_dir: Option<String>,

        /// Path to Google service account credentials JSON (for gdrive backend)
        #[arg(long, env = "FIO_GDRIVE_CREDENTIALS")]
        gdrive_credentials: Option<String>,

        /// Google Drive folder spec (path, id:..., or drive:...)
        #[arg(long, env = "FIO_GDRIVE_FOLDER")]
        gdrive_folder: Option<String>,

        /// Domain-wide delegation user for Google service account
        #[arg(long, env = "FIO_GDRIVE_IMPERSONATE")]
        gdrive_impersonate: Option<String>,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// List configured accounts from config file
    ListAccounts,
}

/// Account entry in the config file.
#[derive(Debug, serde::Deserialize)]
struct AccountConfig {
    #[serde(default)]
    name: String,
    account_id: String,
    token: String,
}

/// Top-level config file structure.
#[derive(Debug, serde::Deserialize)]
struct Config {
    #[serde(default)]
    accounts: Vec<AccountConfig>,
}

fn load_config() -> Result<Config> {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("fio-fetcher");
    let config_path = config_dir.join("config.toml");

    if !config_path.exists() {
        return Ok(Config {
            accounts: Vec::new(),
        });
    }

    let content = std::fs::read_to_string(&config_path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

fn find_token_for_account(
    config: &Config,
    account_id: &str,
    cli_token: Option<String>,
) -> Result<String> {
    if let Some(token) = cli_token {
        return Ok(token);
    }

    // Search in config by account_id or name
    for account in &config.accounts {
        if account.account_id == account_id || account.name == account_id {
            return Ok(account.token.clone());
        }
    }

    anyhow::bail!(
        "No token found for account '{}'. Set FIO_TOKEN env var, pass --token, or configure in ~/.config/fio-fetcher/config.toml",
        account_id
    )
}

fn create_storage(
    backend: &str,
    account_id: &str,
    message_dir: Option<&str>,
    _gdrive_credentials: Option<&str>,
    _gdrive_folder: Option<&str>,
    _gdrive_impersonate: Option<&str>,
) -> Result<Box<dyn TransactionStorage>> {
    match backend {
        "filesystem" => {
            let base_dir = match message_dir {
                Some(dir) => PathBuf::from(dir),
                None => dirs::data_local_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("fio-fetcher"),
            };
            Ok(Box::new(storage::FilesystemStorage::new(
                base_dir, account_id,
            )))
        }
        #[cfg(feature = "gdrive")]
        "gdrive" | "gdrive-sheets" => {
            let credentials_path = _gdrive_credentials.ok_or_else(|| {
                anyhow::anyhow!("--gdrive-credentials required for gdrive storage")
            })?;
            let folder_name = _gdrive_folder
                .ok_or_else(|| anyhow::anyhow!("--gdrive-folder required for gdrive storage"))?;
            let spreadsheet_name = format!("FIO Account {}", account_id);

            let storage = storage::GDriveStorage::new(
                PathBuf::from(credentials_path).as_path(),
                folder_name,
                &spreadsheet_name,
                account_id,
                Some(account_id),
                None,
                _gdrive_impersonate,
            )?;

            Ok(Box::new(storage))
        }
        #[cfg(not(feature = "gdrive"))]
        "gdrive" | "gdrive-sheets" => {
            anyhow::bail!(
                "Google Drive storage requires the 'gdrive' feature. Rebuild with --features gdrive"
            );
        }
        other => {
            anyhow::bail!(
                "Unknown storage backend: '{}'. Use 'filesystem' or 'gdrive'",
                other
            );
        }
    }
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => log::LevelFilter::Warn,
        1 => log::LevelFilter::Info,
        _ => log::LevelFilter::Debug,
    };

    env_logger::Builder::new()
        .filter_level(level)
        .format_timestamp_secs()
        .init();
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| anyhow::anyhow!("Invalid date '{}': {}. Use YYYY-MM-DD format.", s, e))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    match cli.command {
        Commands::FetchAccount {
            account_id,
            token,
            from_date,
            to_date,
            limit,
            storage,
            message_dir,
            gdrive_credentials,
            gdrive_folder,
            gdrive_impersonate,
            json,
        } => {
            let config = load_config()?;
            let token = find_token_for_account(&config, &account_id, token)?;

            let to = match &to_date {
                Some(s) => parse_date(s)?,
                None => chrono::Local::now().date_naive(),
            };

            let client = FioApiClient::new(&token);

            // Determine from_date: explicit > auto-resume from storage
            let from = match &from_date {
                Some(s) => parse_date(s)?,
                None => {
                    let storage = create_storage(
                        &storage,
                        &account_id,
                        message_dir.as_deref(),
                        gdrive_credentials.as_deref(),
                        gdrive_folder.as_deref(),
                        gdrive_impersonate.as_deref(),
                    )?;
                    match storage.get_latest_transaction_date()? {
                        Some(date) => {
                            log::info!("Auto-resuming from last known date: {}", date);
                            date
                        }
                        None => {
                            let default_from = to - chrono::Duration::days(30);
                            log::info!(
                                "No previous transactions found, defaulting to {} (30 days ago)",
                                default_from
                            );
                            default_from
                        }
                    }
                }
            };

            let mut storage = create_storage(
                &storage,
                &account_id,
                message_dir.as_deref(),
                gdrive_credentials.as_deref(),
                gdrive_folder.as_deref(),
                gdrive_impersonate.as_deref(),
            )?;

            let results =
                fetcher::fetch_transactions(&client, storage.as_mut(), &from, &to, limit, json)?;

            let stored = results
                .iter()
                .filter(|r| matches!(r.status, fetcher::FetchStatus::Stored))
                .count();
            let skipped = results
                .iter()
                .filter(|r| matches!(r.status, fetcher::FetchStatus::Skipped))
                .count();
            let failed = results
                .iter()
                .filter(|r| matches!(r.status, fetcher::FetchStatus::Failed(_)))
                .count();

            log::info!(
                "Done: {} stored, {} skipped, {} failed",
                stored,
                skipped,
                failed
            );
        }
        Commands::ListAccounts => {
            let config = load_config()?;
            if config.accounts.is_empty() {
                println!("No accounts configured.");
                println!();
                println!("Add accounts to ~/.config/fio-fetcher/config.toml:");
                println!();
                println!("[[accounts]]");
                println!("name = \"my-account\"");
                println!("token = \"your-fio-api-token\"");
            } else {
                println!("Configured FIO accounts:");
                println!();
                for account in &config.accounts {
                    let token_preview = if account.token.len() > 8 {
                        format!(
                            "{}...{}",
                            &account.token[..4],
                            &account.token[account.token.len() - 4..]
                        )
                    } else {
                        "*".repeat(account.token.len())
                    };
                    println!("  {} (token: {})", account.name, token_preview);
                }
            }
        }
    }

    Ok(())
}
