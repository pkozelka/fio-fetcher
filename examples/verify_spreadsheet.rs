//! Verify that a Google Sheets spreadsheet exists and is accessible.
//!
//! Usage: `cargo run --example verify-spreadsheet --features gdrive -- <credentials> <folder> <spreadsheet-name>`

use anyhow::Result;
use fio_fetcher::storage::GDriveStorage;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: verify-spreadsheet <credentials-json> <folder-spec> <spreadsheet-name>");
        eprintln!("  folder-spec: path, id:<id>, or drive:<name>/path");
        std::process::exit(1);
    }

    let credentials_path = &args[1];
    let folder_spec = &args[2];
    let spreadsheet_name = &args[3];

    let storage = GDriveStorage::new(
        std::path::Path::new(credentials_path),
        folder_spec,
        spreadsheet_name,
        "verify-test",
        None,
        None,
        None,
    )?;

    // Try to ensure the spreadsheet exists (will create if needed)
    let spreadsheet_id = storage.ensure_spreadsheet()?;
    println!(
        "Spreadsheet '{}' is accessible (id: {})",
        spreadsheet_name, spreadsheet_id
    );

    Ok(())
}
