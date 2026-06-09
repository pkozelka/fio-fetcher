# fio-fetcher

CLI to fetch Czech FIO banka transactions via their REST API and store them locally or in Google Sheets. Designed for cron-based automated transaction retrieval.

## Features

- Fetches account transactions from FIO Banka REST API
- Supports date range queries (`--from-date` / `--to-date`) and auto-resume from last known transaction
- Stores transactions as JSONL files organized by year/month
- Incremental runs — only downloads new transactions (`--from-date auto`)
- Plain text or JSON output (`--json` flag)
- Environment variable support for tokens (cron-friendly)
- Multi-account support via config file (`list-accounts` subcommand)
- **Google Sheets storage** (feature-gated) — stores transactions in a Google Sheets spreadsheet with year tabs

## Installation

### From source

```bash
git clone https://github.com/pkozelka/fio-fetcher.git
cd fio-fetcher
make install
```

### Using Cargo

```bash
cargo install --git https://github.com/pkozelka/fio-fetcher.git
```

### With Google Sheets support

```bash
cargo install --git https://github.com/pkozelka/fio-fetcher.git --features gdrive
```

## Prerequisites

- A FIO Banka API token (available in your FIO internet banking under Settings → API)
- For Google Sheets storage: a Google Workspace service account JSON key file

## Configuration

### Single account (CLI flags)

Pass the token directly:

```bash
fio-fetcher fetch-account 2301234567 --token YOUR_FIO_TOKEN --from-date 2024-01-01
```

### Multi-account config file

Create `~/.config/fio-fetcher/config.toml`:

```toml
[[accounts]]
name = "Personal"
account_id = "2301234567"
token = "your-fio-api-token-here"

[[accounts]]
name = "Business"
account_id = "2309876543"
token = "another-fio-api-token-here"
```

Then use `list-accounts` or pass the account ID:

```bash
fio-fetcher list-accounts
fio-fetcher fetch-account 2301234567 --from-date auto
```

When using the config file, the token is read from the config (no `--token` needed).

### Environment variables

```bash
export FIO_TOKEN=your-fio-api-token
export FIO_FROM_DATE=2024-01-01
export FIO_TO_DATE=2024-12-31
fio-fetcher fetch-account 2301234567
```

## Usage

### Fetch transactions (filesystem storage)

```bash
fio-fetcher fetch-account 2301234567 \
  --token YOUR_TOKEN \
  --from-date 2024-01-01 \
  --to-date 2024-12-31 \
  --message-dir ./transactions
```

### Fetch only new transactions (incremental)

```bash
fio-fetcher fetch-account 2301234567 \
  --token YOUR_TOKEN \
  --from-date auto
```

The `auto` start date scans existing JSONL files to find the latest transaction date and only fetches transactions after that date.

### Fetch with Google Sheets storage

```bash
fio-fetcher fetch-account 2301234567 \
  --token YOUR_TOKEN \
  --from-date auto \
  --storage gdrive \
  --gdrive-credentials ./google-credentials.json \
  --gdrive-folder "Fio/2301234567"
```

### JSON output

```bash
fio-fetcher fetch-account 2301234567 --token TOKEN --json
```

### List configured accounts

```bash
fio-fetcher list-accounts
```

## CLI Reference

### `fetch-account <ACCOUNT-ID> [options]`

| Flag | Env var | Default | Description |
|------|---------|---------|-------------|
| `--token` | `FIO_TOKEN` | from config | FIO Banka API token |
| `--from-date` | `FIO_FROM_DATE` | `auto` | Start date (YYYY-MM-DD) or `auto` to resume |
| `--to-date` | `FIO_TO_DATE` | today | End date (YYYY-MM-DD) |
| `--limit` | — | `0` | Max transactions to process (0 = unlimited) |
| `--storage` | `FIO_STORAGE` | `filesystem` | Storage backend: `filesystem` or `gdrive` |
| `--message-dir` | `FIO_MESSAGE_DIR` | XDG data dir | Local storage directory |
| `--gdrive-credentials` | `FIO_GDRIVE_CREDENTIALS` | `./google-credentials.json` | Service account JSON path |
| `--gdrive-folder` | `FIO_GDRIVE_FOLDER` | `Fio/{ACCOUNT_ID}` | Google Drive folder spec |
| `--gdrive-impersonate` | `FIO_GDRIVE_IMPERSONATE` | — | Domain-wide delegation user |
| `--json` | — | `false` | Output as JSON |
| `-v` / `-vv` | — | warn | Verbosity level |

### `list-accounts [options]`

Lists all accounts from `~/.config/fio-fetcher/config.toml`.

## Storage Layout

### Filesystem

```
transactions/
└── 2301234567/                  # Account ID
    ├── index.json                # Account metadata
    ├── 2024/
    │   ├── 2024-01.jsonl         # One JSON line per transaction
    │   ├── 2024-02.jsonl
    │   └── ...
    └── 2025/
        └── 2025-01.jsonl
```

Each line in the JSONL file is a JSON object with all transaction fields.

### Google Sheets

- **Overview tab**: Account info header, balance, last sync date
- **Year tabs** (e.g. "2024"): columns A-I = Date, Amount, Currency, Counter Account, Counter Account Name, VS, KS, SS, Comment

## FIO Banka API

The FIO Banka REST API (v1) provides read-only access to account transactions.

| Endpoint | Purpose |
|----------|---------|
| `periods/{token}/{from}/{to}/transactions.json` | Transactions by date range |
| `last/{token}/transactions.json` | Transactions since last download marker |
| `set-last-id/{token}/{id}` | Set last downloaded ID pointer |
| `set-last-date/{token}/{yyyy-mm-dd}` | Set last downloaded date pointer |

- **Base URL**: `https://fioapi.fio.cz/v1/rest/`
- **Token**: 64-character string, per-account, available in FIO internet banking
- **Rate limit**: Minimum 30 seconds between requests per token

## Development

```bash
make build           # Build release binary
make test            # Run tests
make check           # Run clippy
make clean           # Clean build artifacts
make install         # Install to ~/.local/bin/
make build-gdrive    # Build with Google Sheets support
```

## License

MIT