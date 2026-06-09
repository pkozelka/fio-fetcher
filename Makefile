.PHONY: build build-gdrive test check clean install run run-gdrive require-FIO_TOKEN

-include .env
export FIO_TOKEN
export FIO_FROM_DATE
export FIO_TO_DATE
export FIO_STORAGE
export FIO_MESSAGE_DIR
export FIO_GDRIVE_CREDENTIALS
export FIO_GDRIVE_FOLDER
export FIO_GDRIVE_IMPERSONATE
# Override via: make run FETCHER_ARGS="-vv --limit 1"
FETCHER_ARGS ?= -v

# -- Require targets (validate required env vars) ────────────────────

require-FIO_TOKEN:
	# FIO_TOKEN is required
	@test -n "$(FIO_TOKEN)"

# ── Build targets ───────────────────────────────────────────────────

build:
	cargo build --release

test:
	cargo test

check:
	cargo check
	cargo clippy -- -D warnings

clean:
	cargo clean

install: build
	install -m 755 target/release/fio-fetcher $(HOME)/.local/bin/

build-gdrive:
	cargo build --release --features gdrive

# ── Run targets ─────────────────────────────────────────────────────

# Run locally (filesystem storage)
# Requires: FIO_TOKEN, FIO_FROM_DATE (or auto-resume)
run: build require-FIO_TOKEN
	./target/release/fio-fetcher fetch-account auto --storage filesystem $(FETCHER_ARGS)

# Run locally with Google Sheets storage
# Requires: FIO_TOKEN, FIO_GDRIVE_CREDENTIALS
# Optional: FIO_GDRIVE_FOLDER (default: Fio/{ACCOUNT_ID})
#   Supports: path "Fio/origis", id "id:FOLDER_ID", shared drive "drive:MyDrive/Sub"
#   FIO_GDRIVE_IMPERSONATE (user email for domain-wide delegation)
run-gdrive: build-gdrive require-FIO_TOKEN
	test -n "$(FIO_GDRIVE_CREDENTIALS)"  # FIO_GDRIVE_CREDENTIALS is required for gdrive
	./target/release/fio-fetcher fetch-account auto --storage gdrive $(FETCHER_ARGS)

# ── Test targets ────────────────────────────────────────────────────

# Integration test against FIO production API
# Requires: FIO_TOKEN, FIO_FROM_DATE
test-fio: build require-FIO_TOKEN
	./target/release/fio-fetcher fetch-account auto --message-dir /tmp/fio-test-output $(FETCHER_ARGS)
	ls -la /tmp/fio-test-output/ 2>/dev/null || echo "No output directory created"
	find /tmp/fio-test-output -name "*.jsonl" -exec echo "--- {} ---" \; -exec head -5 {} \; 2>/dev/null || true