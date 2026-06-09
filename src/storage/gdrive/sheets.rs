#![allow(unknown_lints)]
#![allow(clippy::collapsible_if)]

//! Google Sheets REST API operations: spreadsheet management, tabs, and formatting.
//!
//! FIO-specific layout:
//! - Overview tab: account number, currency, IBAN, BIC, balance info, last sync
//! - Year tabs: columns A-I = Date, Amount, Currency, Counter Account, Counter Account Name, VS, KS, SS, Comment

use anyhow::{Context, Result};
use log::{debug, info, warn};

use super::GDriveStorage;
use super::types::*;
use super::{DRIVE_API_URL, SHEETS_API_URL};
use chrono::NaiveDate;

impl GDriveStorage {
    // ── Sheets API helpers ─────────────────────────────────────────────

    /// Find a spreadsheet by name within the spreadsheet parent folder.
    pub(super) fn find_spreadsheet(&self, name: &str) -> Result<Option<String>> {
        let parent_id = self.spreadsheet_parent()?;
        let token = self.get_access_token()?;
        let query = format!(
            "name='{}' and '{}' in parents and mimeType='application/vnd.google-apps.spreadsheet' and trashed=false",
            name.replace('\'', "\\'"),
            parent_id,
        );
        let url = format!(
            "{}/files?q={}&includeItemsFromAllDrives=true&supportsAllDrives=true&fields=files(id,name)",
            DRIVE_API_URL,
            urlencoding::encode(&query),
        );

        let client = self.http_client();
        let list: DriveFileList = client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .with_context(|| format!("Failed to search for spreadsheet '{}'", name))?
            .error_for_status()
            .with_context(|| format!("Spreadsheet search for '{}' returned error", name))?
            .json()
            .with_context(|| "Failed to parse spreadsheet search response")?;

        Ok(list.files.into_iter().next().map(|f| f.id))
    }

    /// Create a new spreadsheet and move it into the spreadsheet parent folder.
    pub(super) fn create_spreadsheet(&self, name: &str) -> Result<String> {
        let parent_id = self.spreadsheet_parent()?;
        let token = self.get_access_token()?;
        let body = serde_json::json!({
            "properties": {
                "title": name,
                "locale": "en_US",
            },
        });

        let client = self.http_client();
        let sp: Spreadsheet = client
            .post(SHEETS_API_URL)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .with_context(|| format!("Failed to create spreadsheet '{}'", name))?
            .error_for_status()
            .with_context(|| format!("Spreadsheet creation for '{}' returned error", name))?
            .json()
            .with_context(|| "Failed to parse spreadsheet creation response")?;

        // Move the spreadsheet into the parent folder
        let move_url = format!(
            "{}/files/{}?addParents={}&supportsAllDrives=true",
            DRIVE_API_URL, sp.spreadsheet_id, parent_id
        );
        let client = self.http_client();
        let move_response = client
            .patch(&move_url)
            .bearer_auth(&token)
            .json(&serde_json::json!({}))
            .send()
            .with_context(|| format!("Failed to move spreadsheet '{}' into parent folder", name))?
            .error_for_status()
            .with_context(|| "Drive file move returned error")?;
        let _ = move_response.text();

        info!(
            "Created spreadsheet '{}' (id={}) in folder '{}'",
            name, sp.spreadsheet_id, self.root_folder_name
        );
        Ok(sp.spreadsheet_id)
    }

    /// Fix the locale of an existing spreadsheet to en_US.
    pub(super) fn fix_spreadsheet_locale(&self, spreadsheet_id: &str) -> Result<()> {
        let token = self.get_access_token()?;
        let url = format!("{}/{}:batchUpdate", SHEETS_API_URL, spreadsheet_id);

        let body = serde_json::json!({
            "requests": [{
                "updateSpreadsheetProperties": {
                    "properties": { "locale": "en_US" },
                    "fields": "locale"
                }
            }]
        });

        let client = self.http_client();
        let resp = client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .with_context(|| "Failed to update spreadsheet locale")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            warn!(
                "Locale fix for spreadsheet {} returned {}: {}",
                spreadsheet_id, status, body
            );
        } else {
            debug!("Ensured spreadsheet {} locale is en_US", spreadsheet_id);
        }
        Ok(())
    }

    /// Find or create the index spreadsheet.
    pub fn ensure_spreadsheet(&self) -> Result<String> {
        {
            let guard = self.spreadsheet_id.lock().unwrap();
            if let Some(ref id) = *guard {
                return Ok(id.clone());
            }
        }

        let spreadsheet_id = match self.find_spreadsheet(&self.spreadsheet_name)? {
            Some(id) => {
                info!(
                    "Found existing spreadsheet '{}' (id={})",
                    self.spreadsheet_name, id
                );
                if let Err(e) = self.fix_spreadsheet_locale(&id) {
                    warn!("Failed to fix spreadsheet locale: {}", e);
                }
                id
            }
            None => {
                let id = self.create_spreadsheet(&self.spreadsheet_name)?;
                if let Err(e) = self.setup_overview_tab(&id) {
                    warn!("Failed to set up Overview tab: {}", e);
                }
                id
            }
        };

        {
            let mut guard = self.spreadsheet_id.lock().unwrap();
            *guard = Some(spreadsheet_id.clone());
        }

        Ok(spreadsheet_id)
    }

    /// Append rows to a sheet tab.
    pub(super) fn batch_append_rows(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        rows: &[Vec<String>],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let token = self.get_access_token()?;
        let url = format!(
            "{}/{}/values/{}!A:A:append?valueInputOption=USER_ENTERED&insertDataOption=OVERWRITE",
            SHEETS_API_URL,
            spreadsheet_id,
            urlencoding::encode(sheet_name)
        );

        let body = serde_json::json!({ "values": rows });

        let client = self.http_client();
        let response = client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .with_context(|| {
                format!(
                    "Failed to batch-append {} rows to sheet '{}'",
                    rows.len(),
                    sheet_name
                )
            })?;

        let status = response.status();
        if status.is_success() {
            info!(
                "Batch-appended {} rows to sheet '{}' in spreadsheet {}",
                rows.len(),
                sheet_name,
                spreadsheet_id
            );
            Ok(())
        } else {
            let error_body = response.text().unwrap_or_default();
            if error_body.contains("Unable to parse range") {
                debug!("Sheet '{}' not found, creating it", sheet_name);
                self.create_sheet_tab(spreadsheet_id, sheet_name)?;
                self.batch_append_rows(spreadsheet_id, sheet_name, rows)?;
                self.add_year_to_overview(spreadsheet_id, sheet_name)?;
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "Failed to batch-append {} rows to sheet '{}': {} - {}",
                    rows.len(),
                    sheet_name,
                    status,
                    error_body
                ))
            }
        }
    }

    /// Set up the Overview tab in a newly created spreadsheet.
    ///
    /// FIO-specific layout:
    /// Row 1: Title "FIO Account {account_id}", "Last sync" in C1, date in D1
    /// Row 2: "Account: {account_id}", "Currency: {currency}", "IBAN: {iban}", "BIC: {bic}"
    /// Row 3: Column headers — Year | Transactions | Balance | Latest
    pub(super) fn setup_overview_tab(&self, spreadsheet_id: &str) -> Result<()> {
        let token = self.get_access_token()?;

        // Step 1: Rename default "Sheet1" to "Overview"
        let rename_url = format!("{}/{}:batchUpdate", SHEETS_API_URL, spreadsheet_id);
        let rename_body = serde_json::json!({
            "requests": [{
                "updateSheetProperties": {
                    "properties": { "sheetId": 0, "title": "Overview" },
                    "fields": "title"
                }
            }]
        });

        let client = self.http_client();
        let rename_resp = client
            .post(&rename_url)
            .bearer_auth(&token)
            .json(&rename_body)
            .send()
            .with_context(|| "Failed to rename default sheet to 'Overview'")?;
        if !rename_resp.status().is_success() {
            let status = rename_resp.status();
            let body = rename_resp.text().unwrap_or_default();
            warn!("Rename sheet tab returned {}: {}", status, body);
        }

        debug!("Renamed default sheet tab to 'Overview'");

        // Step 2: Write header rows
        let header_range = "Overview!A1:D3";
        let header_url = format!(
            "{}/{}/values/{}?valueInputOption=USER_ENTERED",
            SHEETS_API_URL,
            spreadsheet_id,
            urlencoding::encode(header_range)
        );

        let title = match &self.account_name {
            Some(name) if !name.is_empty() => format!("FIO Account {}", name),
            _ => format!("FIO Account {}", self.account_id),
        };

        let account_label = format!("Account: {}", self.account_id);
        let currency_label = self
            .account_currency
            .as_deref()
            .map(|c| format!("Currency: {}", c))
            .unwrap_or_default();

        let values = vec![
            vec![
                title,
                String::new(),
                "Last sync".to_string(),
                chrono::Local::now().format("%Y-%m-%d").to_string(),
            ],
            vec![account_label, currency_label, String::new(), String::new()],
            vec![
                "Year".to_string(),
                "Transactions".to_string(),
                "Balance".to_string(),
                "Latest".to_string(),
            ],
        ];

        let client = self.http_client();
        let data_resp = client
            .put(&header_url)
            .bearer_auth(&token)
            .json(&serde_json::json!({ "values": values }))
            .send()
            .with_context(|| "Failed to write Overview tab data")?;
        if !data_resp.status().is_success() {
            let status = data_resp.status();
            let body = data_resp.text().unwrap_or_default();
            warn!("Overview tab data write returned {}: {}", status, body);
        }

        // Step 3: Format — bold title, italic metadata, bold header row, frozen rows/columns
        let format_url = format!("{}/{}:batchUpdate", SHEETS_API_URL, spreadsheet_id);
        let format_body = serde_json::json!({
            "requests": [
                // Bold title row (row 1)
                {
                    "repeatCell": {
                        "range": { "sheetId": 0, "startRowIndex": 0, "endRowIndex": 1 },
                        "cell": {
                            "userEnteredFormat": { "textFormat": { "bold": true, "fontSize": 14 } }
                        },
                        "fields": "userEnteredFormat(textFormat)"
                    }
                },
                // Italic metadata row (row 2)
                {
                    "repeatCell": {
                        "range": { "sheetId": 0, "startRowIndex": 1, "endRowIndex": 2 },
                        "cell": {
                            "userEnteredFormat": { "textFormat": { "italic": true, "fontSize": 10 } }
                        },
                        "fields": "userEnteredFormat(textFormat)"
                    }
                },
                // Bold header row with light gray background (row 3)
                {
                    "repeatCell": {
                        "range": { "sheetId": 0, "startRowIndex": 2, "endRowIndex": 3 },
                        "cell": {
                            "userEnteredFormat": {
                                "textFormat": { "bold": true },
                                "backgroundColor": { "red": 0.94, "green": 0.94, "blue": 0.94 }
                            }
                        },
                        "fields": "userEnteredFormat(textFormat,backgroundColor)"
                    }
                },
                // Freeze row 1-3 and column A
                {
                    "updateSheetProperties": {
                        "properties": {
                            "sheetId": 0,
                            "gridProperties": { "frozenRowCount": 3, "frozenColumnCount": 1 }
                        },
                        "fields": "gridProperties.frozenRowCount,gridProperties.frozenColumnCount"
                    }
                },
                // Format column D (Latest) as datetime
                {
                    "repeatCell": {
                        "range": { "sheetId": 0, "startRowIndex": 3, "startColumnIndex": 3, "endColumnIndex": 4 },
                        "cell": {
                            "userEnteredFormat": {
                                "numberFormat": { "type": "DATE_TIME", "pattern": "yyyy-mm-dd HH:mm" }
                            }
                        },
                        "fields": "userEnteredFormat(numberFormat)"
                    }
                },
                // Format D1 as date
                {
                    "repeatCell": {
                        "range": { "sheetId": 0, "startRowIndex": 0, "endRowIndex": 1, "startColumnIndex": 3, "endColumnIndex": 4 },
                        "cell": {
                            "userEnteredFormat": {
                                "numberFormat": { "type": "DATE", "pattern": "yyyy-mm-dd" }
                            }
                        },
                        "fields": "userEnteredFormat(numberFormat)"
                    }
                },
                // Set column widths
                {
                    "updateDimensionProperties": {
                        "range": { "sheetId": 0, "dimension": "COLUMNS", "startIndex": 0, "endIndex": 4 },
                        "properties": { "pixelSize": 160 },
                        "fields": "pixelSize"
                    }
                }
            ]
        });

        let client = self.http_client();
        let fmt_resp = client
            .post(&format_url)
            .bearer_auth(&token)
            .json(&format_body)
            .send()
            .with_context(|| "Failed to format Overview tab")?;
        if !fmt_resp.status().is_success() {
            let status = fmt_resp.status();
            let body = fmt_resp.text().unwrap_or_default();
            warn!(
                "Overview tab format batch update returned {}: {}",
                status, body
            );
        }

        info!("Set up Overview tab in spreadsheet {}", spreadsheet_id);
        Ok(())
    }

    /// Create a new sheet tab for a year with FIO-specific columns.
    ///
    /// Columns: A=Date, B=Amount, C=Currency, D=Counter Account, E=Counter Account Name,
    ///          F=VS, G=KS, H=SS, I=Comment
    pub(super) fn create_sheet_tab(&self, spreadsheet_id: &str, sheet_name: &str) -> Result<()> {
        let token = self.get_access_token()?;
        let url = format!("{}/{}:batchUpdate", SHEETS_API_URL, spreadsheet_id);

        // Step 1: Create the sheet tab and get its sheet ID
        let add_body = serde_json::json!({
            "requests": [{
                "addSheet": {
                    "properties": { "title": sheet_name }
                }
            }]
        });

        let client = self.http_client();
        let response = client
            .post(&url)
            .bearer_auth(&token)
            .json(&add_body)
            .send()
            .with_context(|| format!("Failed to create sheet tab '{}'", sheet_name))?
            .error_for_status()
            .with_context(|| format!("Sheet tab creation for '{}' returned error", sheet_name))?;

        let resp_text = response
            .text()
            .with_context(|| "Failed to read sheet tab creation response")?;

        let resp_val: serde_json::Value = serde_json::from_str(&resp_text).unwrap_or_default();
        let sheet_id: i32 = resp_val
            .get("replies")
            .and_then(|r| r.get(0))
            .and_then(|r| r.get("addSheet"))
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.get("sheetId"))
            .and_then(|id| id.as_i64())
            .map(|id| id as i32)
            .unwrap_or(0);

        debug!(
            "Created sheet tab '{}' (sheetId={}) in spreadsheet {}",
            sheet_name, sheet_id, spreadsheet_id
        );

        // Step 2: Write header row (9 columns: A-I)
        let header_range = format!("{}!A1:I1", sheet_name);
        let header_url = format!(
            "{}/{}/values/{}?valueInputOption=USER_ENTERED",
            SHEETS_API_URL,
            spreadsheet_id,
            urlencoding::encode(&header_range)
        );
        let header_values = vec![vec![
            "Date".to_string(),
            "Amount".to_string(),
            "Currency".to_string(),
            "Counter Account".to_string(),
            "Counter Account Name".to_string(),
            "VS".to_string(),
            "KS".to_string(),
            "SS".to_string(),
            "Comment".to_string(),
        ]];

        let client = self.http_client();
        let header_resp = client
            .put(&header_url)
            .bearer_auth(&token)
            .json(&serde_json::json!({ "values": header_values }))
            .send()
            .with_context(|| format!("Failed to write header row for sheet '{}'", sheet_name))?;
        if !header_resp.status().is_success() {
            let status = header_resp.status();
            let body = header_resp.text().unwrap_or_default();
            warn!(
                "Header row write for sheet '{}' returned {} {}",
                sheet_name, status, body
            );
        }

        // Step 3: Format header row + freeze + column widths
        let format_body = serde_json::json!({
            "requests": [
                // Bold header row with light gray background
                {
                    "repeatCell": {
                        "range": { "sheetId": sheet_id, "startRowIndex": 0, "endRowIndex": 1 },
                        "cell": {
                            "userEnteredFormat": {
                                "textFormat": { "bold": true },
                                "backgroundColor": { "red": 0.94, "green": 0.94, "blue": 0.94 }
                            }
                        },
                        "fields": "userEnteredFormat(textFormat,backgroundColor)"
                    }
                },
                // Freeze row 1 (header) and column A (Date)
                {
                    "updateSheetProperties": {
                        "properties": {
                            "sheetId": sheet_id,
                            "gridProperties": { "frozenRowCount": 1, "frozenColumnCount": 1 }
                        },
                        "fields": "gridProperties.frozenRowCount,gridProperties.frozenColumnCount"
                    }
                },
                // Format column A (Date) as date
                {
                    "repeatCell": {
                        "range": { "sheetId": sheet_id, "startRowIndex": 1, "startColumnIndex": 0, "endColumnIndex": 1 },
                        "cell": {
                            "userEnteredFormat": {
                                "numberFormat": { "type": "DATE", "pattern": "yyyy-mm-dd" }
                            }
                        },
                        "fields": "userEnteredFormat(numberFormat)"
                    }
                },
                // Format column B (Amount) as number with 2 decimal places
                {
                    "repeatCell": {
                        "range": { "sheetId": sheet_id, "startRowIndex": 1, "startColumnIndex": 1, "endColumnIndex": 2 },
                        "cell": {
                            "userEnteredFormat": {
                                "numberFormat": { "type": "NUMBER", "pattern": "#,##0.00" }
                            }
                        },
                        "fields": "userEnteredFormat(numberFormat)"
                    }
                },
                // Set column widths: A=100, B=100, C=60, D=120, E=200, F=100, G=80, H=80, I=250
                { "updateDimensionProperties": { "range": { "sheetId": sheet_id, "dimension": "COLUMNS", "startIndex": 0, "endIndex": 1 }, "properties": { "pixelSize": 100 }, "fields": "pixelSize" } },
                { "updateDimensionProperties": { "range": { "sheetId": sheet_id, "dimension": "COLUMNS", "startIndex": 1, "endIndex": 2 }, "properties": { "pixelSize": 100 }, "fields": "pixelSize" } },
                { "updateDimensionProperties": { "range": { "sheetId": sheet_id, "dimension": "COLUMNS", "startIndex": 2, "endIndex": 3 }, "properties": { "pixelSize": 60 }, "fields": "pixelSize" } },
                { "updateDimensionProperties": { "range": { "sheetId": sheet_id, "dimension": "COLUMNS", "startIndex": 3, "endIndex": 4 }, "properties": { "pixelSize": 120 }, "fields": "pixelSize" } },
                { "updateDimensionProperties": { "range": { "sheetId": sheet_id, "dimension": "COLUMNS", "startIndex": 4, "endIndex": 5 }, "properties": { "pixelSize": 200 }, "fields": "pixelSize" } },
                { "updateDimensionProperties": { "range": { "sheetId": sheet_id, "dimension": "COLUMNS", "startIndex": 5, "endIndex": 6 }, "properties": { "pixelSize": 100 }, "fields": "pixelSize" } },
                { "updateDimensionProperties": { "range": { "sheetId": sheet_id, "dimension": "COLUMNS", "startIndex": 6, "endIndex": 7 }, "properties": { "pixelSize": 80 }, "fields": "pixelSize" } },
                { "updateDimensionProperties": { "range": { "sheetId": sheet_id, "dimension": "COLUMNS", "startIndex": 7, "endIndex": 8 }, "properties": { "pixelSize": 80 }, "fields": "pixelSize" } },
                { "updateDimensionProperties": { "range": { "sheetId": sheet_id, "dimension": "COLUMNS", "startIndex": 8, "endIndex": 9 }, "properties": { "pixelSize": 250 }, "fields": "pixelSize" } }
            ]
        });

        let client = self.http_client();
        let resp3 = client
            .post(&url)
            .bearer_auth(&token)
            .json(&format_body)
            .send()
            .with_context(|| format!("Failed to format header row for sheet '{}'", sheet_name))?;
        if !resp3.status().is_success() {
            let status = resp3.status();
            let body = resp3.text().unwrap_or_default();
            warn!(
                "Format batch update for sheet '{}' returned {}: {}",
                sheet_name, status, body
            );
        }

        debug!(
            "Applied header formatting and column widths to sheet '{}'",
            sheet_name
        );
        Ok(())
    }

    /// Add a year summary row to the Overview tab.
    pub(super) fn add_year_to_overview(&self, spreadsheet_id: &str, year: &str) -> Result<()> {
        let token = self.get_access_token()?;
        let check_url = format!("{}/{}/values/Overview!A:A", SHEETS_API_URL, spreadsheet_id);
        let client = self.http_client();
        let resp = client
            .get(&check_url)
            .bearer_auth(&token)
            .send()
            .with_context(|| "Failed to read Overview tab data")?;

        let mut next_row: u32 = 4; // Row 1=title, Row 2=metadata, Row 3=header, data starts at row 4

        let vals = match resp.json::<serde_json::Value>() {
            Ok(v) => v,
            Err(_) => {
                debug!("Could not parse Overview tab response, will add year row");
                serde_json::Value::Null
            }
        };
        if let Some(columns) = vals.get("values").and_then(|v| v.as_array()) {
            for row in columns.iter() {
                let row_arr = match row.as_array() {
                    Some(arr) => arr,
                    None => continue,
                };
                let cell_a = row_arr.first().and_then(|c| c.as_str()).unwrap_or("");
                if cell_a == year {
                    debug!("Year {} already present in Overview tab", year);
                    return Ok(());
                }
            }
            next_row = (columns.len() as u32 + 1).max(4);
        }

        // Write the new row: A=Year, B=COUNTA (transactions count), C=latest closing balance, D=Latest date
        let range = format!("Overview!A{}:D{}", next_row, next_row);
        let put_url = format!(
            "{}/{}/values/{}?valueInputOption=USER_ENTERED",
            SHEETS_API_URL,
            spreadsheet_id,
            urlencoding::encode(&range)
        );
        // B: COUNTA of Amount column in year tab (non-empty = has data)
        let transactions_counta = format!("=COUNTA('{}'!B2:B)", year);
        let body = serde_json::json!({
            "values": [[year, transactions_counta, "", ""]]
        });

        let client = self.http_client();
        let put_resp = client
            .put(&put_url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .with_context(|| "Failed to write year row to Overview tab")?;

        if put_resp.status().is_success() {
            info!(
                "Added year {} row to Overview tab at row {}",
                year, next_row
            );
        } else {
            let status = put_resp.status();
            let error_body = put_resp.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to write year {} row to Overview tab: {} {}",
                year,
                status,
                error_body
            );
        }

        Ok(())
    }

    /// Update the latest date in the Overview tab for a given year.
    pub(super) fn update_overview_latest_date(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        datetime: &str,
    ) -> Result<()> {
        debug!(
            "update_overview_latest_date called for sheet {} with datetime {}",
            sheet_name, datetime
        );
        let token = self.get_access_token()?;
        let check_url = format!("{}/{}/values/Overview!A:A", SHEETS_API_URL, spreadsheet_id);
        let client = self.http_client();
        let resp = client
            .get(&check_url)
            .bearer_auth(&token)
            .send()
            .with_context(|| "Failed to read Overview tab for latest date update")?;

        let mut target_row: Option<u32> = None;
        let vals = match resp.json::<serde_json::Value>() {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };
        if let Some(columns) = vals.get("values").and_then(|v| v.as_array()) {
            for (i, row) in columns.iter().enumerate() {
                let row_arr = match row.as_array() {
                    Some(arr) => arr,
                    None => continue,
                };
                let cell_a = row_arr.first().and_then(|c| c.as_str()).unwrap_or("");
                if cell_a == sheet_name {
                    target_row = Some(i as u32 + 1);
                    break;
                }
            }
        }

        let row_num = match target_row {
            Some(r) => r,
            None => {
                debug!(
                    "Year {} not found in Overview tab — skipping latest date update",
                    sheet_name
                );
                return Ok(());
            }
        };

        let range = format!("Overview!D{}", row_num);
        let put_url = format!(
            "{}/{}/values/{}?valueInputOption=USER_ENTERED",
            SHEETS_API_URL,
            spreadsheet_id,
            urlencoding::encode(&range)
        );
        let body = serde_json::json!({ "values": [[datetime]] });
        let client = self.http_client();
        let put_resp = client
            .put(&put_url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .with_context(|| "Failed to update latest date in Overview tab")?;

        if put_resp.status().is_success() {
            debug!(
                "Updated latest date for {} in Overview tab at row {} to '{}'",
                sheet_name, row_num, datetime
            );
        } else {
            let status = put_resp.status();
            let error_body = put_resp.text().unwrap_or_default();
            warn!(
                "Failed to update latest date for {}: {} {}",
                sheet_name, status, error_body
            );
        }

        Ok(())
    }

    /// Update the "Last sync" date in cell D1 of the Overview tab.
    pub(super) fn update_last_sync_date(&self, spreadsheet_id: &str, date: &str) -> Result<()> {
        debug!("update_last_sync_date called with date {}", date);
        let token = self.get_access_token()?;
        let url = format!(
            "{}/{}/values/Overview!D1?valueInputOption=USER_ENTERED",
            SHEETS_API_URL, spreadsheet_id
        );
        let body = serde_json::json!({ "values": [[date]] });
        let client = self.http_client();
        let resp = client
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .with_context(|| "Failed to update last sync date")?;

        if resp.status().is_success() {
            debug!("Updated last sync date to {} in Overview tab", date);
        } else {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            warn!("Failed to update last sync date: {} {}", status, body);
        }
        Ok(())
    }

    /// Query the sheet for a transaction ID to check if it exists.
    pub(super) fn query_sheet_for_transaction_id(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        transaction_id: i64,
    ) -> Result<bool> {
        let token = self.get_access_token()?;
        // We'll search column A for the transaction ID
        // FIO transactions use numeric IDs, stored as plain values (not HYPERLINK formulas)
        let id_str = transaction_id.to_string();
        let range = format!("{}!A:A", sheet_name);
        let url = format!(
            "{}/{}/values/{}?majorDimension=COLUMNS",
            SHEETS_API_URL,
            spreadsheet_id,
            urlencoding::encode(&range)
        );

        let client = self.http_client();
        let response = client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .with_context(|| {
                format!("Failed to query sheet '{}' for transaction ID", sheet_name)
            })?;

        if !response.status().is_success() {
            debug!(
                "Sheet '{}' query failed, assuming transaction doesn't exist",
                sheet_name
            );
            return Ok(false);
        }

        let vr: ValueRange = response
            .json()
            .with_context(|| "Failed to parse sheet query response")?;

        if let Some(columns) = &vr.values {
            if let Some(col_a) = columns.first() {
                return Ok(col_a.iter().any(|v| {
                    // Match by plain value or as part of HYPERLINK formula
                    if v.starts_with('=') {
                        v.contains(&id_str)
                    } else {
                        *v == id_str
                    }
                }));
            }
        }

        Ok(false)
    }

    /// Get the latest transaction date from a year sheet.
    pub(super) fn get_latest_date_from_sheet(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
    ) -> Result<Option<NaiveDate>> {
        let token = self.get_access_token()?;
        // Column A = Date in our FIO layout
        let range = format!("{}!A:A", sheet_name);
        let url = format!(
            "{}/{}/values/{}?majorDimension=COLUMNS",
            SHEETS_API_URL,
            spreadsheet_id,
            urlencoding::encode(&range)
        );

        let client = self.http_client();
        let response = client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .with_context(|| format!("Failed to query sheet '{}' for dates", sheet_name))?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let vr: ValueRange = response
            .json()
            .with_context(|| "Failed to parse sheet date query response")?;

        if let Some(columns) = &vr.values {
            if let Some(col_a) = columns.first() {
                // Skip header row, parse dates
                let latest = col_a
                    .iter()
                    .skip(1)
                    .filter_map(|v| {
                        NaiveDate::parse_from_str(
                            &v.chars().take(10).collect::<String>(),
                            "%Y-%m-%d",
                        )
                        .ok()
                    })
                    .max();
                return Ok(latest);
            }
        }

        Ok(None)
    }
}
