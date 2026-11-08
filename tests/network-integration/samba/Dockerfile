FROM docker.io/library/alpine:3.20

RUN apk add --no-cache samba samba-common-tools

COPY smb.conf /etc/samba/smb.conf
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 445

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
