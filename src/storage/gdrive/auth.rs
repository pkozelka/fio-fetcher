//! OAuth2 service account authentication via JWT grant flow.

use anyhow::{Context, Result};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use log::debug;

use super::GDriveStorage;
use super::types::*;
use super::{SCOPES, TOKEN_URL};

impl GDriveStorage {
    /// Obtain or refresh an OAuth2 access token using JWT grant flow.
    ///
    /// If `impersonate_user` is set, the JWT includes `sub` for domain-wide delegation.
    pub(super) fn get_access_token(&self) -> Result<String> {
        {
            let token_guard = self.access_token.lock().unwrap();
            let expires_guard = self.token_expires_at.lock().unwrap();
            if let Some(ref token) = *token_guard {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs();
                // Refresh 60 seconds before expiry
                if *expires_guard > now + 60 && !token.is_empty() {
                    return Ok(token.clone());
                }
            }
        }

        debug!("Refreshing Google OAuth2 access token");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let claims = JwtClaims {
            iss: self.credentials.client_email.clone(),
            scope: SCOPES.to_string(),
            aud: self
                .credentials
                .token_uri
                .clone()
                .unwrap_or_else(|| TOKEN_URL.to_string()),
            iat: now,
            exp: now + 3600,
            sub: self.impersonate_user.clone(),
        };

        let header = Header {
            kid: Some(self.credentials.private_key_id.clone()),
            alg: Algorithm::RS256,
            ..Default::default()
        };

        let key = EncodingKey::from_rsa_pem(self.credentials.private_key.as_bytes())
            .with_context(|| "Failed to parse RSA private key from service account credentials")?;

        let jwt = encode(&header, &claims, &key)
            .with_context(|| "Failed to create JWT for Google OAuth2")?;

        // Exchange JWT for access token
        let client = self.http_client();
        let response: TokenResponse = client
            .post(&claims.aud)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &jwt),
            ])
            .send()
            .with_context(|| "Failed to send OAuth2 token request to Google")?
            .error_for_status()
            .with_context(|| "Google OAuth2 token exchange failed")?
            .json()
            .with_context(|| "Failed to parse Google OAuth2 token response")?;

        debug!(
            "Obtained Google access token (expires in {}s)",
            response.expires_in
        );

        let expires_at = now + response.expires_in;
        {
            let mut token_guard = self.access_token.lock().unwrap();
            let mut expires_guard = self.token_expires_at.lock().unwrap();
            *token_guard = Some(response.access_token.clone());
            *expires_guard = expires_at;
        }

        Ok(response.access_token)
    }
}
