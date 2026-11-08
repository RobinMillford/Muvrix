#!/bin/sh
set -e

# Create Linux user if not exists
if ! id muvrix_smb >/dev/null 2>&1; then
    adduser -D -s /sbin/nologin muvrix_smb
fi

# Set Samba password
(echo "SmbPassword123!"; echo "SmbPassword123!") | smbpasswd -a -s muvrix_smb
smbpasswd -e muvrix_smb

mkdir -p /srv/media /srv/guest_media

echo "[Samba] Starting smbd service on port 445 (SMB2_02 - SMB3_11)..."
exec smbd -F --no-process-group -d 1 -s /etc/samba/smb.conf
