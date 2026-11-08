#!/bin/sh
set -e

# Create htpasswd credentials
htpasswd -b -c -B /etc/lighttpd/htpasswd muvrix_dav DavPassword123!
chmod 644 /etc/lighttpd/htpasswd

# Ensure webdav root exists
mkdir -p /var/www/webdav

echo "[WebDAV] Starting lighttpd WebDAV service on port 8080..."
exec lighttpd -D -f /etc/lighttpd/webdav.conf
