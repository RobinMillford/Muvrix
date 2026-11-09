//! Remote network storage providers, credential isolation, and streaming URL resolution.

pub mod credentials;
pub mod provider;
pub mod sftp;
pub mod smb;
pub mod webdav;
pub mod webdav_xml;

pub use crate::database::NetworkConnection;
pub use credentials::{
    generate_credential_ref, redact_credentials_from_url, CredentialError, CredentialVault,
    MUVRIX_KEYRING_SERVICE,
};
pub use provider::{
    MockNetworkProvider, NetworkError, NetworkProvider, NetworkResult, RemoteFileEntry,
};
pub use sftp::{PresentedHostKey, SftpProvider};
pub use smb::SmbProvider;
pub use webdav::WebDavProvider;
pub use webdav_xml::parse_propfind_xml;
