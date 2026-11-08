FROM docker.io/library/alpine:3.20

RUN apk add --no-cache lighttpd lighttpd-mod_auth lighttpd-mod_webdav apache2-utils

COPY webdav.conf /etc/lighttpd/webdav.conf
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
