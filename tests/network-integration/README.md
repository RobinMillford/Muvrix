# Muvrix Network Integration Testing Environment

This directory provisions a local, isolated, reproducible rootless Podman testing environment for validating Muvrix against actual network storage services:
1. **WebDAV**: Lighttpd RFC 4918 server with HTTP Basic Authentication and byte-range support.
2. **Samba (SMB2/3)**: Samba 4 `smbd` daemon supporting SMB 2.0.2 to 3.1.1 dialect negotiation, NTLM authentication, and direct tree connects.
3. **OpenSSH (SFTP)**: Hardened OpenSSH 9.7 daemon with an ephemeral Ed25519 host key and internal-sftp subsystem.

---

## Security Boundaries & Guarantees

1. **Loopback Only**: All services bind exclusively to `127.0.0.1`. Ports are never exposed to the LAN or public networks.
2. **Ephemeral Host Keys**: The OpenSSH container generates a fresh, ephemeral Ed25519 host key at container startup. No private host keys exist in the repository or container image layers.
3. **Dynamic Fingerprint Resolution**: The public host key fingerprint is queried dynamically from the running container by `setup.sh` and exposed to test runners via `sftp_fingerprint.txt` and `MUVRIX_TEST_SFTP_FINGERPRINT`.
4. **Hardened SFTP Containment**: The OpenSSH daemon enforces `ForceCommand internal-sftp`, `AllowTcpForwarding no`, `PermitTTY no`, `X11Forwarding no`, `AllowUsers muvrix_sftp`, and assigns `/sbin/nologin` as the user shell. Interactive shell sessions and TCP port tunneling are rejected.
5. **Client Public-Key Authentication**: Client public-key authentication is validated by dynamically generating ephemeral Ed25519 client key pairs during test execution and injecting public keys into the container's `authorized_keys`.
6. **Keyring Safety & Isolation**: Native Secret Service integration tests are guarded with RAII `KeyringTestGuard` drop handlers to guarantee zero leftover secrets even on panic, and are gated behind `MUVRIX_TEST_NATIVE_KEYRING=1` to prevent polluting workstation keyrings during routine unit testing.
7. **Automated Teardown**: The teardown script (`teardown.sh`) reliably stops and removes containers and ephemeral fingerprint files, even after test failures.

---

## Endpoint Configuration

| Protocol | Loopback Endpoint | Share / Path | Test User | Test Password | Host Key / Security |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **WebDAV** | `http://127.0.0.1:18080` | `/webdav/` | `muvrix_dav` | `DavPassword123!` | Basic Auth, HTTP 206 Range |
| **Samba** | `smb://127.0.0.1:18445` | `/media` (and `/guest_media`) | `muvrix_smb` | `SmbPassword123!` | SMB 2.0.2 - 3.1.1, NTLM |
| **SFTP** | `sftp://127.0.0.1:18222` | `/media` | `muvrix_sftp` | `SftpPassword123!` | Ephemeral Ed25519 (Dynamic Fingerprint) |

---

## Usage

```bash
# 1. Start containers and generate fixtures
./tests/network-integration/scripts/setup.sh

# 2. Run the integration test suite
./tests/network-integration/scripts/run.sh

# 3. Teardown containers
./tests/network-integration/scripts/teardown.sh
```
