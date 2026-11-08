//! WebDAV remote network provider for HTTP/HTTPS directory traversal and direct libmpv streaming.

use super::credentials::{base64_encode, redact_credentials_from_url, CredentialVault};
use super::provider::{NetworkError, NetworkProvider, NetworkResult, RemoteFileEntry};

/// Concrete WebDAV provider supporting standard HTTP and HTTPS endpoints.
#[derive(Debug, Clone)]
pub struct WebDavProvider {
    pub host: String,
    pub port: u16,
    pub remote_path: String,
    pub use_https: bool,
    pub username: Option<String>,
    pub credential_ref: Option<String>,
    pub vault: CredentialVault,
}

impl WebDavProvider {
    pub fn new(
        host: &str,
        port: u16,
        remote_path: &str,
        use_https: bool,
        username: Option<String>,
        credential_ref: Option<String>,
        vault: CredentialVault,
    ) -> Self {
        Self {
            host: host.trim().to_string(),
            port,
            remote_path: remote_path.trim().to_string(),
            use_https,
            username,
            credential_ref,
            vault,
        }
    }

    /// Formats the base server endpoint URI (e.g. `https://nas.local:8443`).
    pub fn base_url(&self) -> String {
        let scheme = if self.use_https { "https" } else { "http" };
        let default_port = if self.use_https { 443 } else { 80 };
        if self.port == default_port || self.port == 0 {
            format!("{}://{}", scheme, self.host)
        } else {
            format!("{}://{}:{}", scheme, self.host, self.port)
        }
    }

    /// Parses an RFC 4918 PROPFIND XML response payload.
    pub fn parse_propfind(
        &self,
        xml_response: &str,
        requested_path: &str,
    ) -> NetworkResult<Vec<RemoteFileEntry>> {
        super::webdav_xml::parse_propfind_xml(xml_response, requested_path)
    }

    /// Formats an RFC 4918 PROPFIND XML request body for querying collection children.
    pub fn propfind_request_body() -> &'static str {
        r#"<?xml version="1.0" encoding="utf-8" ?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
    <D:resourcetype/>
    <D:getcontentlength/>
    <D:getlastmodified/>
    <D:getetag/>
  </D:prop>
</D:propfind>"#
    }
    /// Validates configuration parameters (host, port, path) format without performing network I/O.
    pub fn validate_config(&self) -> NetworkResult<()> {
        if self.host.trim().is_empty() {
            return Err(NetworkError::ConnectionFailed(
                "Host cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    /// Generates raw Basic authorization header value (e.g. `Basic dXNlcjpwYXNz`) without the header name.
    pub fn auth_header_value(&self) -> Option<String> {
        if let (Some(ref user), Some(ref cred_ref)) = (&self.username, &self.credential_ref) {
            if let Some(secret) = self.vault.get(cred_ref) {
                let token = format!("{}:{}", user, secret);
                let encoded = base64_encode(token.as_bytes());
                return Some(format!("Basic {}", encoded));
            }
        }
        None
    }

    /// Generates an HTTP Basic Authorization header string from credentials in the vault,
    /// keeping secrets isolated in memory rather than exposed in playback URLs.
    pub fn auth_header(&self) -> Option<String> {
        self.auth_header_value()
            .map(|val| format!("Authorization: {}", val))
    }
}

impl NetworkProvider for WebDavProvider {
    fn protocol(&self) -> &'static str {
        "webdav"
    }

    /// Tests live reachability, TLS, and credentials against the remote WebDAV server using PROPFIND Depth: 0.
    fn test_connection(&self) -> NetworkResult<bool> {
        self.validate_config()?;

        let base = self.base_url();
        let target_path = if self.remote_path.starts_with('/') {
            &self.remote_path
        } else {
            &format!("/{}", self.remote_path)
        };
        let url = format!("{}{}", base, target_path);

        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(4))
            .build();

        let mut req = agent
            .request("PROPFIND", &url)
            .set("Depth", "0")
            .set("Content-Type", "application/xml; charset=utf-8");

        if let Some(auth) = self.auth_header_value() {
            req = req.set("Authorization", &auth);
        }

        match req.send_string(Self::propfind_request_body()) {
            Ok(resp) => {
                let status = resp.status();
                if status == 200 || status == 207 {
                    Ok(true)
                } else {
                    Err(NetworkError::Protocol(format!(
                        "Unexpected WebDAV HTTP response status: {status}"
                    )))
                }
            }
            Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
                Err(NetworkError::AuthenticationFailed(
                    "WebDAV authentication failed: invalid credentials or permission denied"
                        .to_string(),
                ))
            }
            Err(ureq::Error::Status(404, _)) => Err(NetworkError::NotFound(format!(
                "WebDAV remote path not found: {target_path}"
            ))),
            Err(ureq::Error::Status(code, resp)) => Err(NetworkError::Protocol(format!(
                "WebDAV server returned HTTP {code}: {}",
                resp.status_text()
            ))),
            Err(ureq::Error::Transport(e)) => Err(NetworkError::ConnectionFailed(format!(
                "Failed to connect to WebDAV server at {base}: {e}"
            ))),
        }
    }

    /// Queries the WebDAV collection children via RFC 4918 PROPFIND with `Depth: 1` and parses the 207 Multi-Status XML.
    fn list_directory(&self, path: &str) -> NetworkResult<Vec<RemoteFileEntry>> {
        self.validate_config()?;

        let norm_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        };

        let base = self.base_url();
        let url = format!("{}{}", base, norm_path);

        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(6))
            .build();

        let mut req = agent
            .request("PROPFIND", &url)
            .set("Depth", "1")
            .set("Content-Type", "application/xml; charset=utf-8");

        if let Some(auth) = self.auth_header_value() {
            req = req.set("Authorization", &auth);
        }

        match req.send_string(Self::propfind_request_body()) {
            Ok(resp) => {
                let status = resp.status();
                if status == 207 || status == 200 {
                    let xml = resp.into_string().map_err(|e| {
                        NetworkError::Protocol(format!("Failed to read WebDAV response body: {e}"))
                    })?;
                    self.parse_propfind(&xml, &norm_path)
                } else {
                    Err(NetworkError::Protocol(format!(
                        "WebDAV PROPFIND expected HTTP 207 Multi-Status, got {status}"
                    )))
                }
            }
            Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
                Err(NetworkError::AuthenticationFailed(
                    "WebDAV authentication failed: unauthorized".to_string(),
                ))
            }
            Err(ureq::Error::Status(404, _)) => Err(NetworkError::NotFound(format!(
                "WebDAV collection not found at: {norm_path}"
            ))),
            Err(ureq::Error::Status(code, resp)) => Err(NetworkError::Protocol(format!(
                "WebDAV server returned HTTP {code}: {}",
                resp.status_text()
            ))),
            Err(ureq::Error::Transport(e)) => Err(NetworkError::ConnectionFailed(format!(
                "WebDAV connection failed to {url}: {e}"
            ))),
        }
    }

    /// Resolves stream URL strictly without credentials to prevent secret leakage in logs,
    /// URIs, or `current-path` properties. Authentication is delivered out-of-band via headers.
    fn resolve_stream_url(&self, remote_file_path: &str) -> NetworkResult<String> {
        let base = self.base_url();
        let path = if remote_file_path.starts_with('/') {
            remote_file_path
        } else {
            &format!("/{}", remote_file_path)
        };

        Ok(format!("{}{}", base, path))
    }
}

impl std::fmt::Display for WebDavProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let base = self.base_url();
        write!(f, "WebDAV ({})", redact_credentials_from_url(&base))
    }
}
