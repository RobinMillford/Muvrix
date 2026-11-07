//! Remote network storage providers, credential isolation, and streaming URL resolution.

pub mod credentials;
pub mod provider;
pub mod webdav;

pub use crate::database::NetworkConnection;
pub use credentials::{redact_credentials_from_url, CredentialVault};
pub use provider::{
    MockNetworkProvider, NetworkError, NetworkProvider, NetworkResult, RemoteFileEntry,
};
pub use webdav::WebDavProvider;
