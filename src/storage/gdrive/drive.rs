//! Google Drive REST API operations: folder management and file uploads.

use anyhow::{Context, Result};
use log::{debug, info};

use super::DRIVE_API_URL;
use super::GDriveStorage;
use super::types::*;

/// Scopes for Drive API queries — either My Drive or a shared drive.
enum DriveScope<'a> {
    /// My Drive — no drive ID scoping needed.
    MyDrive,
    /// Shared drive — queries include corpora=drive&driveId=...
    SharedDrive { drive_id: &'a str },
}

impl GDriveStorage {
    // ── Drive API helpers ──────────────────────────────────────────────

    /// Find a Drive file by name and parent folder (My Drive).
    #[allow(dead_code)]
    pub(super) fn find_file(
        &self,
        name: &str,
        parent_id: &str,
        mime_type: &str,
    ) -> Result<Option<DriveFile>> {
        self.find_file_in_parent(name, parent_id, mime_type, &DriveScope::MyDrive)
    }

    /// Find a Drive file by name in the root of My Drive.
    #[allow(dead_code)]
    pub(super) fn find_file_in_root(
        &self,
        name: &str,
        mime_type: &str,
    ) -> Result<Option<DriveFile>> {
        self.find_file_in_parent(name, "root", mime_type, &DriveScope::MyDrive)
    }

    /// Find or create a folder by name in the My Drive root.
    pub(super) fn ensure_folder_in_root(&self, name: &str) -> Result<String> {
        self.ensure_folder_internal(name, "root", &DriveScope::MyDrive)
    }

    /// Find or create a folder by name under the given parent (My Drive).
    pub(super) fn ensure_folder(&self, name: &str, parent_id: &str) -> Result<String> {
        self.ensure_folder_internal(name, parent_id, &DriveScope::MyDrive)
    }

    // ── Folder spec resolution: id:, drive:, path ─────────────────────

    /// Find or create the root folder by name.
    ///
    /// Supports three folder spec syntaxes:
    /// - **Path** (default): `"Fio/2301234567"` — walks My Drive root → subfolders
    /// - **Folder ID**: `"id:1BxiMVsV0SXj2kP7P5rGNz1cF8e"` — uses the ID directly
    /// - **Shared drive**: `"drive:MyTeamDrive/Fio"` — looks up shared drive,
    ///   then walks sub-path within it
    pub(super) fn ensure_root_folder(&self) -> Result<String> {
        {
            let guard = self.root_folder_id.lock().unwrap();
            if let Some(ref id) = *guard {
                return Ok(id.clone());
            }
        }

        let root_id = if let Some(id) = self.root_folder_name.strip_prefix("id:") {
            let id = id.trim().to_string();
            info!(
                "Resolved Google Drive folder '{}' → id={} (direct)",
                self.root_folder_name, id
            );
            id
        } else if let Some(path) = self.root_folder_name.strip_prefix("drive:") {
            self.resolve_shared_drive_path(path)?
        } else {
            self.resolve_my_drive_path()?
        };

        {
            let mut guard = self.root_folder_id.lock().unwrap();
            *guard = Some(root_id.clone());
        }

        Ok(root_id)
    }

    fn resolve_my_drive_path(&self) -> Result<String> {
        let parts: Vec<&str> = self.root_folder_name.split('/').collect();
        let mut parent_id: Option<String> = None;

        for part in &parts {
            if part.is_empty() {
                continue;
            }
            let folder_id = if let Some(ref pid) = parent_id {
                self.ensure_folder(part, pid)?
            } else {
                self.ensure_folder_in_root(part)?
            };
            parent_id = Some(folder_id);
        }

        let root_id = parent_id.ok_or_else(|| {
            anyhow::anyhow!(
                "Could not resolve Google Drive folder path '{}'",
                self.root_folder_name
            )
        })?;

        info!(
            "Resolved Google Drive folder '{}' → id={}",
            self.root_folder_name, root_id
        );
        Ok(root_id)
    }

    fn resolve_shared_drive_path(&self, path: &str) -> Result<String> {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.is_empty() || parts[0].is_empty() {
            anyhow::bail!(
                "Invalid drive: spec '{}': expected format 'drive:SharedDriveName/Sub/Folder'",
                self.root_folder_name
            );
        }

        let drive_name = parts[0];
        let drive_id = self.find_shared_drive_by_name(drive_name)?;
        let scope = DriveScope::SharedDrive {
            drive_id: &drive_id,
        };

        info!("Found shared drive '{}' (id={})", drive_name, drive_id);

        if parts.len() == 1 {
            info!(
                "Resolved Google Drive folder '{}' → id={} (shared drive root)",
                self.root_folder_name, drive_id
            );
            return Ok(drive_id);
        }

        let mut parent_id: String = drive_id.clone();
        for part in &parts[1..] {
            if part.is_empty() {
                continue;
            }
            let p_id = self.ensure_folder_internal(part, &parent_id, &scope)?;
            parent_id = p_id;
        }

        info!(
            "Resolved Google Drive folder '{}' → id={} (shared drive)",
            self.root_folder_name, parent_id
        );
        Ok(parent_id)
    }

    /// Get the folder ID where the spreadsheet should live.
    pub(super) fn spreadsheet_parent(&self) -> Result<String> {
        {
            let guard = self.spreadsheet_parent_id.lock().unwrap();
            if let Some(ref id) = *guard {
                return Ok(id.clone());
            }
        }
        self.ensure_root_folder()
    }

    // ── Unified internal helpers ──────────────────────────────────────

    fn find_file_in_parent(
        &self,
        name: &str,
        parent_id: &str,
        mime_type: &str,
        scope: &DriveScope,
    ) -> Result<Option<DriveFile>> {
        let cache_key = match scope {
            DriveScope::MyDrive => Self::cache_key(parent_id, name),
            DriveScope::SharedDrive { drive_id } => {
                format!("drive:{}:{}", drive_id, Self::cache_key(parent_id, name))
            }
        };

        if let Some(cached_id) = self.folder_cache_get(&cache_key) {
            debug!("Cache hit for '{}': {}", name, cached_id);
            return Ok(Some(DriveFile {
                id: cached_id,
                name: name.to_string(),
                mime_type: mime_type.to_string(),
            }));
        }

        let is_root = parent_id == "root";
        let query = if is_root {
            format!(
                "name='{}' and 'root' in parents and mimeType='{}' and trashed=false",
                name.replace('\'', "\\'"),
                mime_type
            )
        } else {
            format!(
                "name='{}' and '{}' in parents and mimeType='{}' and trashed=false",
                name.replace('\'', "\\'"),
                parent_id,
                mime_type
            )
        };

        let mut url = format!(
            "{}/files?q={}&includeItemsFromAllDrives=true&supportsAllDrives=true&fields=files(id,name,mimeType)",
            DRIVE_API_URL,
            urlencoding::encode(&query),
        );

        if let DriveScope::SharedDrive { drive_id } = scope {
            url.push_str(&format!("&corpora=drive&driveId={}", drive_id));
        }

        let token = self.get_access_token()?;
        let client = self.http_client();
        let list: DriveFileList = client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .with_context(|| format!("Failed to search Google Drive for '{}'", name))?
            .error_for_status()
            .with_context(|| format!("Google Drive search for '{}' returned error", name))?
            .json()
            .with_context(|| "Failed to parse Drive file list response")?;

        if let Some(file) = list.files.into_iter().next() {
            self.folder_cache_put(cache_key, file.id.clone());
            Ok(Some(file))
        } else {
            Ok(None)
        }
    }

    fn create_folder_internal(&self, name: &str, parent_id: Option<&str>) -> Result<DriveFile> {
        let token = self.get_access_token()?;
        let url = format!(
            "{}{}?supportsAllDrives=true&fields=id,name,mimeType",
            DRIVE_API_URL, "/files"
        );
        let mut body = serde_json::json!({
            "name": name,
            "mimeType": "application/vnd.google-apps.folder",
        });
        if let Some(pid) = parent_id {
            body["parents"] = serde_json::json!([pid]);
        }

        let client = self.http_client();
        let file: DriveFile = client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .with_context(|| format!("Failed to create folder '{}' in Google Drive", name))?
            .error_for_status()
            .with_context(|| format!("Google Drive folder creation for '{}' returned error", name))?
            .json()
            .with_context(|| "Failed to parse Drive file creation response")?;

        if let Some(pid) = parent_id {
            info!(
                "Created Google Drive folder '{}' (id={}) under {}",
                name, file.id, pid
            );
            self.folder_cache_put(Self::cache_key(pid, name), file.id.clone());
        } else {
            info!(
                "Created Google Drive root folder '{}' (id={})",
                name, file.id
            );
            self.folder_cache_put(Self::cache_key_root(name), file.id.clone());
        }
        Ok(file)
    }

    fn ensure_folder_internal(
        &self,
        name: &str,
        parent_id: &str,
        scope: &DriveScope,
    ) -> Result<String> {
        let folder_mime = "application/vnd.google-apps.folder";
        match self.find_file_in_parent(name, parent_id, folder_mime, scope)? {
            Some(f) => Ok(f.id),
            None => Ok(self.create_folder_internal(name, Some(parent_id))?.id),
        }
    }

    fn find_shared_drive_by_name(&self, name: &str) -> Result<String> {
        let token = self.get_access_token()?;
        let url = format!(
            "{}/drives?q={}&fields=drives(id,name)",
            DRIVE_API_URL,
            urlencoding::encode(&format!("name='{}'", name.replace('\'', "\\'")))
        );

        let client = self.http_client();
        let resp: serde_json::Value = client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .with_context(|| format!("Failed to search for shared drive '{}'", name))?
            .error_for_status()
            .with_context(|| format!("Shared drive search for '{}' returned error", name))?
            .json()
            .with_context(|| "Failed to parse shared drive list response")?;

        let drives = resp
            .get("drives")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();

        if let Some(drive) = drives.into_iter().next() {
            drive
                .get("id")
                .and_then(|id| id.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Shared drive '{}' found but missing ID", name))
        } else {
            anyhow::bail!(
                "Shared drive '{}' not found. Ensure it exists and the service account has access.",
                name
            )
        }
    }

    // ── File uploads ───────────────────────────────────────────────────

    /// Upload a file to Google Drive.
    #[allow(dead_code)]
    pub(super) fn upload_file_to_drive(
        &self,
        filename: &str,
        content: &[u8],
        parent_id: &str,
        mime_type: &str,
    ) -> Result<String> {
        let token = self.get_access_token()?;

        if content.len() < 5 * 1024 * 1024 {
            self.upload_file_simple(&token, filename, content, parent_id, mime_type)
        } else {
            self.upload_file_resumable(&token, filename, content, parent_id, mime_type)
        }
    }

    #[allow(dead_code)]
    fn upload_file_simple(
        &self,
        token: &str,
        filename: &str,
        content: &[u8],
        parent_id: &str,
        mime_type: &str,
    ) -> Result<String> {
        let url = "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&supportsAllDrives=true&fields=id"
            .to_string();
        let metadata = serde_json::json!({
            "name": filename,
            "parents": [parent_id],
        });

        let client = self.http_client();
        let response = client
            .post(&url)
            .bearer_auth(token)
            .multipart(
                reqwest::blocking::multipart::Form::new()
                    .part(
                        "metadata",
                        reqwest::blocking::multipart::Part::text(metadata.to_string())
                            .mime_str("application/json")
                            .with_context(|| "Failed to set metadata mime type")?,
                    )
                    .part(
                        "file",
                        reqwest::blocking::multipart::Part::bytes(content.to_vec())
                            .mime_str(mime_type)
                            .with_context(|| format!("Failed to set mime type for '{}'", filename))?
                            .file_name(filename.to_string()),
                    ),
            )
            .send()
            .with_context(|| format!("Failed to upload file '{}' to Google Drive", filename))?
            .error_for_status()
            .with_context(|| format!("Google Drive upload of '{}' returned error", filename))?
            .json::<serde_json::Value>()
            .with_context(|| "Failed to parse Drive upload response")?;

        let file_id = response["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing file ID in Drive upload response"))?
            .to_string();

        debug!(
            "Uploaded '{}' ({} bytes) → id={}",
            filename,
            content.len(),
            file_id
        );
        Ok(file_id)
    }

    #[allow(dead_code)]
    fn upload_file_resumable(
        &self,
        token: &str,
        filename: &str,
        content: &[u8],
        parent_id: &str,
        mime_type: &str,
    ) -> Result<String> {
        let url = "https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&supportsAllDrives=true";
        let metadata = serde_json::json!({
            "name": filename,
            "parents": [parent_id],
        });

        let client = self.http_client();
        let session_response = client
            .post(url)
            .bearer_auth(token)
            .header("Content-Type", "application/json")
            .body(metadata.to_string())
            .send()
            .with_context(|| format!("Failed to initiate resumable upload for '{}'", filename))?
            .error_for_status()
            .with_context(|| {
                format!(
                    "Resumable upload initiation for '{}' returned error",
                    filename
                )
            })?;

        let upload_url = session_response
            .headers()
            .get("location")
            .ok_or_else(|| {
                anyhow::anyhow!("Missing 'location' header in resumable upload session")
            })?
            .to_str()
            .with_context(|| "Invalid 'location' header in resumable upload response")?
            .to_string();

        let file_id: String = client
            .put(&upload_url)
            .bearer_auth(token)
            .header("Content-Type", mime_type)
            .header("Content-Length", content.len().to_string())
            .body(content.to_vec())
            .send()
            .with_context(|| format!("Failed to upload content for '{}'", filename))?
            .error_for_status()
            .with_context(|| format!("Resumable upload of '{}' returned error", filename))?
            .json::<serde_json::Value>()
            .with_context(|| "Failed to parse resumable upload response")?["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing file ID in resumable upload response"))?
            .to_string();

        debug!(
            "Uploaded '{}' ({} bytes, resumable) → id={}",
            filename,
            content.len(),
            file_id
        );
        Ok(file_id)
    }
}
