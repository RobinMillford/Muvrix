FROM docker.io/library/alpine:3.20

RUN apk add --no-cache openssh-server openssh-sftp-server

COPY sshd_config /etc/ssh/sshd_config
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 2222

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
