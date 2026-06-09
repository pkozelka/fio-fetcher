//! API types for Google Drive and Sheets REST APIs.

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Google Auth: service account JWT types
// ---------------------------------------------------------------------------

/// Parsed service account credentials JSON.
#[derive(Debug, Deserialize, Clone)]
pub(super) struct ServiceAccountCredentials {
    pub(super) client_email: String,
    pub(super) private_key: String,
    /// The private key ID from the JSON key file.
    pub(super) private_key_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) project_id: String,
    #[serde(default)]
    pub(super) token_uri: Option<String>,
}

/// JWT claims for Google OAuth2 service account auth.
#[derive(Debug, serde::Serialize)]
pub(super) struct JwtClaims {
    pub(super) iss: String,
    pub(super) scope: String,
    pub(super) aud: String,
    pub(super) iat: u64,
    pub(super) exp: u64,
    /// Subject for domain-wide delegation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) sub: Option<String>,
}

/// Response from Google's OAuth2 token endpoint.
#[derive(Debug, Deserialize)]
pub(super) struct TokenResponse {
    pub(super) access_token: String,
    /// Seconds until expiry.
    #[serde(default)]
    pub(super) expires_in: u64,
}

// ---------------------------------------------------------------------------
// Google Drive REST API types
// ---------------------------------------------------------------------------

/// A Drive file resource (partial).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mime_type: String,
}

/// Drive file list response.
#[derive(Debug, Deserialize)]
pub struct DriveFileList {
    pub files: Vec<DriveFile>,
}

// ---------------------------------------------------------------------------
// Google Sheets REST API types
// ---------------------------------------------------------------------------

/// A spreadsheet resource (partial).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Spreadsheet {
    pub(super) spreadsheet_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) properties: Option<SpreadsheetProperties>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(super) struct SpreadsheetProperties {
    pub(super) title: String,
}

/// Batch values response from Sheets API.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ValueRange {
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) range: Option<String>,
    pub(super) values: Option<Vec<Vec<String>>>,
}
