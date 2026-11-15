FROM ubuntu:22.04
RUN apt-get update && apt-get install -y --no-install-recommends openssh-server && rm -rf /var/lib/apt/lists/* && mkdir /run/sshd && useradd -m -s /bin/bash fixture && passwd -d fixture && mkdir -p /home/fixture/.ssh && chown -R fixture:fixture /home/fixture/.ssh && chmod 700 /home/fixture/.ssh
COPY sshd_config /etc/ssh/sshd_config
CMD ["sh", "-c", "install -m 600 -o fixture -g fixture /fixtures/public_key /home/fixture/.ssh/authorized_keys && ssh-keygen -A && exec /usr/sbin/sshd -D -e"]
