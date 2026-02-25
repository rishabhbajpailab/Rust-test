//! Bitwarden Secrets Manager client (identical across services).
//!
//! See `postgres-service/src/secrets.rs` for full documentation.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

pub struct SecretsClient {
    access_token: Option<String>,
    api_url: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct BwsSecretResponse {
    value: String,
}

impl SecretsClient {
    pub fn new() -> Self {
        let access_token = std::env::var("BWS_ACCESS_TOKEN").ok();
        let api_url = std::env::var("BWS_API_URL")
            .unwrap_or_else(|_| "https://api.bitwarden.com".to_string());
        Self {
            access_token,
            api_url,
            http: reqwest::Client::new(),
        }
    }

    pub async fn get_secret(&self, secret_id: &str, env_fallback: &str) -> Result<String> {
        if let Some(token) = &self.access_token {
            match self.fetch_from_bitwarden(token, secret_id).await {
                Ok(value) => return Ok(value),
                Err(e) => {
                    tracing::warn!(
                        secret_id,
                        error = %e,
                        "Failed to fetch secret from Bitwarden, falling back to env var"
                    );
                }
            }
        }
        std::env::var(env_fallback).with_context(|| {
            format!(
                "Secret '{secret_id}' not found in Bitwarden and env var '{env_fallback}' is not set"
            )
        })
    }

    async fn fetch_from_bitwarden(&self, token: &str, secret_id: &str) -> Result<String> {
        let url = format!("{}/secrets/{}", self.api_url, secret_id);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .context("HTTP request to Bitwarden Secrets Manager failed")?;

        if !resp.status().is_success() {
            return Err(anyhow!("Bitwarden API returned status {}", resp.status()));
        }

        let body: BwsSecretResponse = resp
            .json()
            .await
            .context("Failed to parse Bitwarden response")?;
        Ok(body.value)
    }
}

pub async fn get_secret(secret_id: &str, env_fallback: &str) -> Result<String> {
    SecretsClient::new().get_secret(secret_id, env_fallback).await
}
