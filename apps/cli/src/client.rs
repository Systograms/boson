use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde_json::Value;

#[derive(Clone)]
pub struct AdminClient {
    http: Client,
    server: String,
    admin_token: Option<String>,
}

impl AdminClient {
    #[must_use]
    pub fn new(server: String, admin_token: Option<String>) -> Self {
        Self {
            http: Client::new(),
            server,
            admin_token,
        }
    }

    #[must_use]
    pub fn admin_token(&self) -> Option<&str> {
        self.admin_token.as_deref()
    }

    pub async fn get_public(&self, endpoint: &str) -> Result<(reqwest::StatusCode, Value)> {
        let url = format!("{}/{}", self.server.trim_end_matches('/'), endpoint);
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("cannot connect to {url}; is boson-server running?"))?;
        let status = response.status();
        let body = response.json().await.unwrap_or(Value::Null);
        Ok((status, body))
    }

    pub async fn get(&self, endpoint: &str) -> Result<Value> {
        let token = self
            .admin_token
            .as_deref()
            .context("admin token required; pass --admin-token or set BOSON_ADMIN_TOKEN")?;
        let url = format!("{}/admin/v1/{endpoint}", self.server.trim_end_matches('/'));
        let response = self.http.get(url).bearer_auth(token).send().await?;
        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() {
            bail!("Admin API returned {status}: {body}");
        }
        Ok(body)
    }

    pub async fn post(&self, endpoint: &str, body: Value) -> Result<Value> {
        let token = self
            .admin_token
            .as_deref()
            .context("admin token required; pass --admin-token or set BOSON_ADMIN_TOKEN")?;
        let url = format!("{}/admin/v1/{endpoint}", self.server.trim_end_matches('/'));
        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() {
            bail!("Admin API returned {status}: {body}");
        }
        Ok(body)
    }
}
