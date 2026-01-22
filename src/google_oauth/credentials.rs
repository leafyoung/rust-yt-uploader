use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{self};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tracing::warn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_uri: String,
    pub scopes: Vec<String>,
    pub expires_in: i64,
}

impl Credentials {
    /// Check if the credentials are valid (not expired)
    pub fn is_valid(&self) -> bool {
        self.expires_in > 600 // More than 10 minutes remaining
    }

    /// Check if credentials have the required scopes
    pub fn has_scopes(&self, scopes: &[&str]) -> bool {
        let required_scopes_set: HashSet<_> = scopes.iter().map(|s| s.to_string()).collect();
        let creds_scopes_set: HashSet<_> = self.scopes.iter().cloned().collect();

        if !required_scopes_set.is_subset(&creds_scopes_set) {
            warn!("Existing credentials lack required scopes, re-authenticating");
            false
        } else {
            true
        }
    }

    /// Convert to JSON format
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| anyhow!("Failed to serialize credentials: {}", e))
    }

    /// Load from JSON file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        serde_json::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse credentials file: {}", e))
    }
}
