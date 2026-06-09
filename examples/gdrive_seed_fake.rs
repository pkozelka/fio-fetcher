//! Seed a Google Sheets spreadsheet with fake FIO transaction data for testing.
//!
//! Usage: `cargo run --example gdrive-seed-fake --features gdrive -- <credentials> <folder> <account-id>`

use anyhow::Result;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: gdrive-seed-fake <credentials-json> <folder-spec> <account-id>");
        eprintln!("  folder-spec: path, id:<id>, or drive:<name>/path");
        std::process::exit(1);
    }

    println!("gdrive-seed-fake example: not yet fully implemented");
    println!(
        "Would seed fake data for account '{}' in folder '{}'",
        args[3], args[2]
    );
    Ok(())
}
