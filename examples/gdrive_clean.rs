//! Clean (delete) a Google Sheets spreadsheet used by fio-fetcher.
//!
//! Usage: `cargo run --example gdrive-clean --features gdrive -- <credentials> <folder> <spreadsheet-name>`

use anyhow::Result;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: gdrive-clean <credentials-json> <folder-spec> <spreadsheet-name>");
        eprintln!("  folder-spec: path, id:<id>, or drive:<name>/path");
        std::process::exit(1);
    }

    println!("gdrive-clean example: not yet fully implemented");
    println!(
        "Would delete spreadsheet '{}' from folder '{}'",
        args[3], args[2]
    );
    Ok(())
}
