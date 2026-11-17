//! Remote network storage providers, credential isolation, and streaming URL resolution.

pub mod credentials;
pub mod media_server;
pub mod nfs;
pub mod pool;
pub mod provider;
pub mod sftp;
pub mod smb;
pub mod webdav;
pub mod webdav_xml;

pub use crate::database::NetworkConnection;
pub use credentials::{
    credential_ref_username, generate_credential_ref, redact_credentials_from_url, CredentialError,
    CredentialVault, MUVRIX_KEYRING_SERVICE,
};
pub use media_server::{
    JellyfinCatalogResponse, JellyfinItem, JellyfinMediaSource, JellyfinProvider,
    JellyfinSystemInfo, PlexDirectory, PlexLibraryContainer, PlexMetadataItem, PlexPartItem,
    PlexProvider, PlexSectionsContainer,
};
pub use nfs::{NfsProvider, NfsUrl, NfsVersion};
pub use pool::{
    ConnectionPool, ConnectionPoolTelemetry, EndpointKey, KeepaliveSweepReport, PoolConfig,
    PooledHandle,
};
pub use provider::{
    MockNetworkProvider, NetworkError, NetworkProvider, NetworkResult, RemoteFileEntry,
};
pub use sftp::{PresentedHostKey, SftpProvider};
pub use smb::SmbProvider;
pub use webdav::WebDavProvider;
pub use webdav_xml::parse_propfind_xml;
