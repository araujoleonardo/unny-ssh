#!/usr/bin/env bash
set -euo pipefail
# Run ONLY inside a dedicated disposable Linux VM. The caller passes a test public key.
[ "${UNNY_DISPOSABLE_DOCKER:-}" = '1' ] || { echo 'Dedicated disposable VM opt-in required.' >&2; exit 1; }
[ "$(id -u)" = '0' ] || exit 1
mkdir -p /opt/unny-fixture /run/sshd
chmod 755 /opt/unny-fixture
cat > /opt/unny-fixture/authorized_keys
chmod 644 /opt/unny-fixture/authorized_keys
for user in unnyfixture unnynodocker; do
  id "$user" >/dev/null 2>&1 || useradd -m -s /bin/sh "$user"
  usermod -p '*' "$user"
done
cat > /opt/unny-fixture/missing-shell <<'SH'
#!/bin/sh
exec /usr/bin/env PATH=/nonexistent /bin/sh "$@"
SH
chmod 755 /opt/unny-fixture/missing-shell
id unnymissing >/dev/null 2>&1 || useradd -m -s /opt/unny-fixture/missing-shell unnymissing
usermod -p '*' unnymissing
usermod -aG docker unnyfixture
ssh-keygen -q -t ed25519 -N '' -f /opt/unny-fixture/host
cat > /opt/unny-fixture/sshd_config <<'CONFIG'
ListenAddress 127.0.0.1
Port 22229
HostKey /opt/unny-fixture/host
PidFile /opt/unny-fixture/sshd.pid
AuthorizedKeysFile /opt/unny-fixture/authorized_keys
AllowUsers unnyfixture unnynodocker unnymissing
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
PermitRootLogin no
StrictModes yes
UsePAM no
AllowAgentForwarding no
AllowTcpForwarding no
X11Forwarding no
LogLevel ERROR
Subsystem sftp internal-sftp
CONFIG
/usr/sbin/sshd -f /opt/unny-fixture/sshd_config
project="/home/unnyfixture/compose's project"
mkdir -p "$project"
cat > "$project/compose.yaml" <<'YAML'
name: unnyfixture
services:
  worker:
    image: busybox:1.37.0
    command: ["sh", "-c", "echo compose-ready; touch /data/retained; while :; do sleep 2; done"]
    volumes:
      - data:/data
volumes:
  data: {}
YAML
printf 'remote original\n' > /home/unnyfixture/note.txt
chown -R unnyfixture:unnyfixture "$project" /home/unnyfixture/note.txt
docker pull busybox:1.37.0
docker volume create unny-empty
docker volume create unny-in-use
docker create --name unny-container-test --mount type=volume,source=unny-in-use,target=/data busybox:1.37.0 sh -c 'echo container-ready; while :; do sleep 2; done'
# Keep a test-only marker so cleanup can identify this VM as a disposable fixture.
printf 'unny tasks 10-12 disposable\n' > /opt/unny-fixture/disposable-marker
