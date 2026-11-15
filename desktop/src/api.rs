//! The HTTP client, with the server certificate pinned by fingerprint.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use reqwest::header::AUTHORIZATION;
use reqwest::{Certificate, Client, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::{normalize_fingerprint, Config};

/// Errors returned by the server API.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The API key was missing, unknown or revoked.
    #[error("unauthorized: the API key was rejected")]
    Unauthorized,
    /// The resource changed on the server since the last synchronisation.
    #[error("conflict: {0}")]
    Conflict(String),
    /// The resource does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// The server refused the request.
    #[error("request refused: {0}")]
    Refused(String),
    /// The server failed.
    #[error("server error: {0}")]
    Server(String),
    /// The connection failed.
    #[error("connection failed: {0}")]
    Network(String),
}

/// The certificate the server publishes.
#[derive(Debug, Clone, Deserialize)]
pub struct CertInfo {
    /// The PEM encoded certificate.
    pub certificate: String,
    /// The SHA-256 fingerprint of the DER certificate.
    pub fingerprint: String,
}

/// The identity behind the API key.
#[derive(Debug, Clone, Deserialize)]
pub struct Me {
    /// The account.
    pub user: MeUser,
    /// The key in use.
    pub key: MeKey,
}

/// The account behind the API key.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct MeUser {
    /// Identifier.
    pub id: i64,
    /// Login name.
    pub username: String,
    /// Display name.
    pub display_name: String,
}

/// The API key in use.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct MeKey {
    /// Identifier.
    pub id: i64,
    /// Label given at creation time.
    pub label: String,
    /// The non secret prefix of the key.
    pub prefix: String,
}

/// A game namespace on the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ServerGame {
    /// Identifier.
    pub id: String,
    /// Name shown in the interface.
    pub name: String,
    /// URL friendly name.
    pub slug: String,
    /// Monotonic version, bumped on every change.
    pub version: i64,
}

/// One file inside a game namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerFile {
    /// Path relative to the namespace.
    pub path: String,
    /// Size in bytes.
    pub size: i64,
    /// SHA-256 of the content.
    pub sha256: String,
    /// Modification time observed by the client, in nanoseconds.
    pub mtime_ns: i64,
    /// Version of this file.
    pub version: i64,
    /// Device that uploaded it.
    pub device: String,
    /// When it was last changed.
    pub updated_at: String,
}

/// The complete state of a game namespace.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// The namespace.
    pub game_id: String,
    /// The version of the namespace.
    pub version: i64,
    /// Every file it holds.
    #[serde(default)]
    pub files: Vec<ServerFile>,
}

/// A client for the Game Save server.
pub struct ApiClient {
    http: Client,
    base: String,
    key: String,
}

impl ApiClient {
    /// Connect to the server, pinning its certificate.
    ///
    /// The certificate is fetched once over an unverified connection, then
    /// checked against the fingerprint the user pinned. Only that exact
    /// certificate is trusted from then on, so a server whose certificate
    /// changed simply stops working instead of being silently accepted.
    pub async fn connect(cfg: &Config) -> Result<Self> {
        if cfg.fingerprint.trim().is_empty() {
            bail!("no certificate fingerprint configured");
        }
        if cfg.api_key.trim().is_empty() {
            bail!("no API key configured");
        }

        let base = cfg.server_url.trim_end_matches('/').to_string();
        let cert_url = format!("{base}/v1/cert");

        let bootstrap = Client::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .context("building the bootstrap HTTP client")?;

        let info: CertInfo = bootstrap
            .get(&cert_url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))
            .context("fetching the server certificate")?
            .json()
            .await
            .context("decoding the server certificate")?;

        let der = pem_to_der(&info.certificate)
            .context("the server returned an unreadable certificate")?;
        let digest = fingerprint_of(&der);

        let expected = normalize_fingerprint(&cfg.fingerprint);
        if !digest.eq_ignore_ascii_case(&expected) {
            bail!(
                "the server certificate does not match the pinned fingerprint\n  expected: {}\n  actual:   {}",
                expected,
                digest
            );
        }
        if !info.fingerprint.eq_ignore_ascii_case(&digest) {
            log::warn!(
                "the server reports the fingerprint {} but its certificate hashes to {}",
                info.fingerprint,
                digest
            );
        }

        let cert = Certificate::from_pem(info.certificate.as_bytes())
            .context("the server certificate could not be loaded")?;
        let http = Client::builder()
            .add_root_certificate(cert)
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .context("building the HTTP client")?;

        Ok(Self {
            http,
            base,
            key: cfg.api_key.clone(),
        })
    }

    /// A short description of the connection, for the interface.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Build a request with the authorisation header already set.
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{}", self.base, path))
            .header(AUTHORIZATION, format!("Bearer {}", self.key))
    }

    /// Read the body of a response, turning the status into an error.
    async fn finish<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        if status.is_success() {
            return resp.json().await.context("decoding the server response");
        }
        let text = resp.text().await.unwrap_or_default();
        let message = extract_message(&text);
        Err(match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ApiError::Unauthorized,
            StatusCode::NOT_FOUND => ApiError::NotFound(message),
            StatusCode::CONFLICT => ApiError::Conflict(message),
            s if s.is_server_error() => ApiError::Server(message),
            _ => ApiError::Refused(message),
        }
        .into())
    }

    /// The identity behind the API key.
    pub async fn me(&self) -> Result<Me> {
        let resp = self.request(reqwest::Method::GET, "/v1/me").send().await?;
        Self::finish(resp).await
    }

    /// Every game of the account.
    pub async fn list_games(&self) -> Result<Vec<ServerGame>> {
        #[derive(Deserialize)]
        struct Wrapper {
            games: Vec<ServerGame>,
        }
        let resp = self
            .request(reqwest::Method::GET, "/v1/games")
            .send()
            .await?;
        let wrapper: Wrapper = Self::finish(resp).await?;
        Ok(wrapper.games)
    }

    /// Register a new game namespace.
    pub async fn create_game(&self, name: &str) -> Result<ServerGame> {
        #[derive(Serialize)]
        struct Body<'a> {
            name: &'a str,
        }
        #[derive(Deserialize)]
        struct Wrapper {
            game: ServerGame,
        }
        let resp = self
            .request(reqwest::Method::POST, "/v1/games")
            .json(&Body { name })
            .send()
            .await?;
        let wrapper: Wrapper = Self::finish(resp).await?;
        Ok(wrapper.game)
    }

    /// Delete a game namespace.
    pub async fn delete_game(&self, game_id: &str) -> Result<()> {
        let resp = self
            .request(reqwest::Method::DELETE, &format!("/v1/games/{game_id}"))
            .send()
            .await?;
        let _ = Self::finish::<serde_json::Value>(resp).await;
        Ok(())
    }

    /// The complete state of a game namespace.
    pub async fn manifest(&self, game_id: &str) -> Result<Manifest> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/v1/games/{game_id}/manifest"),
            )
            .send()
            .await?;
        Self::finish(resp).await
    }

    /// Upload a new version of a file.
    ///
    /// `expect_sha256` is the digest the client believes the server holds. The
    /// server refuses the upload when it does not match, which is how a
    /// conflict is detected.
    pub async fn upload(
        &self,
        game_id: &str,
        path: &str,
        content: &[u8],
        mtime_ns: i64,
        expect_sha256: Option<&str>,
        device: &str,
    ) -> Result<ServerFile> {
        let mut query: Vec<(&str, String)> = vec![
            ("path", path.to_string()),
            ("device", device.to_string()),
            ("mtime_ns", mtime_ns.to_string()),
        ];
        if let Some(expect) = expect_sha256 {
            query.push(("expect_sha256", expect.to_string()));
        }

        #[derive(Deserialize)]
        struct Wrapper {
            file: ServerFile,
        }

        let resp = self
            .request(reqwest::Method::PUT, &format!("/v1/games/{game_id}/file"))
            .query(&query)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(content.to_vec())
            .send()
            .await?;
        let wrapper: Wrapper = Self::finish(resp).await?;
        Ok(wrapper.file)
    }

    /// Download a file.
    pub async fn download(&self, game_id: &str, path: &str) -> Result<Vec<u8>> {
        let resp = self
            .request(reqwest::Method::GET, &format!("/v1/games/{game_id}/file"))
            .query(&[("path", path)])
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let message = extract_message(&text);
            return Err(match status {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ApiError::Unauthorized.into(),
                StatusCode::NOT_FOUND => ApiError::NotFound(message).into(),
                StatusCode::CONFLICT => ApiError::Conflict(message).into(),
                s if s.is_server_error() => ApiError::Server(message).into(),
                _ => ApiError::Refused(message).into(),
            });
        }
        let bytes = resp.bytes().await.context("reading the downloaded file")?;
        Ok(bytes.to_vec())
    }

    /// Delete a file from a game namespace.
    pub async fn delete_file(
        &self,
        game_id: &str,
        path: &str,
        expect_sha256: Option<&str>,
        device: &str,
    ) -> Result<()> {
        let mut query: Vec<(&str, String)> =
            vec![("path", path.to_string()), ("device", device.to_string())];
        if let Some(expect) = expect_sha256 {
            query.push(("expect_sha256", expect.to_string()));
        }
        let resp = self
            .request(
                reqwest::Method::DELETE,
                &format!("/v1/games/{game_id}/file"),
            )
            .query(&query)
            .send()
            .await?;
        let _ = Self::finish::<serde_json::Value>(resp).await;
        Ok(())
    }
}

/// Extract the human readable part of an error response.
fn extract_message(text: &str) -> String {
    #[derive(Deserialize)]
    struct Body {
        error: ErrorBody,
    }
    #[derive(Deserialize)]
    struct ErrorBody {
        message: String,
    }
    match serde_json::from_str::<Body>(text) {
        Ok(body) => body.error.message,
        Err(_) => text.trim().to_string(),
    }
}

/// Decode the first PEM block of a certificate into its DER payload.
pub fn pem_to_der(pem: &str) -> Result<Vec<u8>> {
    let mut body = String::new();
    let mut inside = false;
    for line in pem.lines() {
        let line = line.trim();
        if line.starts_with("-----BEGIN") {
            inside = true;
            continue;
        }
        if line.starts_with("-----END") {
            break;
        }
        if inside {
            body.push_str(line);
        }
    }
    if body.is_empty() {
        return Err(anyhow!("the certificate is empty"));
    }
    base64::engine::general_purpose::STANDARD
        .decode(body.as_bytes())
        .context("the certificate is not valid base64")
}

/// Compute the fingerprint of a DER certificate.
pub fn fingerprint_of(der: &[u8]) -> String {
    normalize_fingerprint(&hex::encode(Sha256::digest(der)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PEM: &str = "-----BEGIN CERTIFICATE-----\n\
                                MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA\n\
                                -----END CERTIFICATE-----\n";

    #[test]
    fn pem_is_decoded() {
        let der = pem_to_der(SAMPLE_PEM).expect("decoding");
        assert!(!der.is_empty());
    }

    #[test]
    fn fingerprint_is_hex_and_colons() {
        let der = pem_to_der(SAMPLE_PEM).unwrap();
        let fp = fingerprint_of(&der);
        assert_eq!(fp.len(), 32 * 3 - 1);
        assert!(fp.contains(':'));
    }
}
