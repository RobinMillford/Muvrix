#!/bin/sh
set -e

# Generate ephemeral Ed25519 host key if missing
if [ ! -f /etc/ssh/ssh_host_ed25519_key ]; then
    echo "[SFTP] Generating fresh ephemeral Ed25519 server host key..."
    ssh-keygen -t ed25519 -f /etc/ssh/ssh_host_ed25519_key -N "" -q
fi
chmod 600 /etc/ssh/ssh_host_ed25519_key
chmod 644 /etc/ssh/ssh_host_ed25519_key.pub

if ! id muvrix_sftp >/dev/null 2>&1; then
    adduser -D -s /sbin/nologin muvrix_sftp
    echo "muvrix_sftp:SftpPassword123!" | chpasswd
fi

mkdir -p /home/muvrix_sftp/media /home/muvrix_sftp/.ssh
chmod 700 /home/muvrix_sftp/.ssh
touch /home/muvrix_sftp/.ssh/authorized_keys
chmod 600 /home/muvrix_sftp/.ssh/authorized_keys
chown -R muvrix_sftp:muvrix_sftp /home/muvrix_sftp 2>/dev/null || true

echo "[SFTP] Starting hardened OpenSSH sshd on port 2222..."
exec /usr/sbin/sshd -D -e -f /etc/ssh/sshd_config
