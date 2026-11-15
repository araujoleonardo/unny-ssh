#!/usr/bin/env bash
set -euo pipefail
# All keys, authorization and sockets belong exclusively to this temporary server.
fixture_dir=$(mktemp -d)
fixture_sshd_pid=""
fixture_agent_pid=""
cleanup() {
  if [ -n "$fixture_sshd_pid" ]; then kill "$fixture_sshd_pid" 2>/dev/null || true; fi
  if [ -n "$fixture_agent_pid" ]; then kill "$fixture_agent_pid" 2>/dev/null || true; fi
  chmod -R u+rwX "$fixture_dir" 2>/dev/null || true
  rm -rf "$fixture_dir"
}
trap cleanup EXIT INT TERM
chmod 700 "$fixture_dir"
ssh-keygen -q -t ed25519 -N '' -f "$fixture_dir/host"
ssh-keygen -q -t ed25519 -N '' -f "$fixture_dir/id_ed25519"
ssh-keygen -q -t rsa -b 2048 -N 'disposable fixture passphrase' -f "$fixture_dir/id_rsa"
cat "$fixture_dir/id_ed25519.pub" "$fixture_dir/id_rsa.pub" > "$fixture_dir/authorized_keys"
fixture_port=$(python3 - <<'PY'
import socket
with socket.socket() as s:
    s.bind(('127.0.0.1',0))
    print(s.getsockname()[1])
PY
)
fixture_user=$(id -un)
fixture_sftp=""
for candidate in /usr/libexec/sftp-server /usr/lib/openssh/sftp-server; do
  if [ -x "$candidate" ]; then fixture_sftp="$candidate"; break; fi
done
if [ -z "$fixture_sftp" ]; then echo 'OpenSSH sftp-server is required for the native fixture.' >&2; exit 1; fi
cat > "$fixture_dir/force-command.sh" <<'SHELL'
#!/bin/sh
case "${SSH_ORIGINAL_COMMAND:-}" in
  /usr/libexec/sftp-server) exec /usr/libexec/sftp-server ;;
  /usr/lib/openssh/sftp-server) exec /usr/lib/openssh/sftp-server ;;
  *) if [ -n "${SSH_ORIGINAL_COMMAND:-}" ]; then exec /bin/sh -c "$SSH_ORIGINAL_COMMAND"; else exec /bin/sh -i; fi ;;
esac
SHELL
chmod 700 "$fixture_dir/force-command.sh"
cat > "$fixture_dir/sshd_config" <<CFG
ListenAddress 127.0.0.1
Port $fixture_port
HostKey $fixture_dir/host
PidFile $fixture_dir/sshd.pid
AuthorizedKeysFile $fixture_dir/authorized_keys
AllowUsers $fixture_user
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
PermitRootLogin prohibit-password
StrictModes no
UsePAM no
AllowAgentForwarding no
AllowTcpForwarding no
X11Forwarding no
LogLevel ERROR
Subsystem sftp $fixture_sftp
ForceCommand $fixture_dir/force-command.sh
CFG
fixture_sshd=$(command -v sshd || true)
if [ -z "$fixture_sshd" ]; then fixture_sshd=/usr/sbin/sshd; fi
"$fixture_sshd" -D -e -f "$fixture_dir/sshd_config" > "$fixture_dir/sshd.log" 2>&1 &
fixture_sshd_pid=$!
ssh-agent -D -a "$fixture_dir/agent.sock" > "$fixture_dir/agent.log" 2>&1 &
fixture_agent_pid=$!
export SSH_AUTH_SOCK="$fixture_dir/agent.sock"
for _ in $(seq 1 50); do
  if [ -S "$SSH_AUTH_SOCK" ]; then break; fi
  sleep 0.1
done
ssh-add "$fixture_dir/id_ed25519" >/dev/null 2>&1
sleep 0.2
if ! kill -0 "$fixture_sshd_pid" 2>/dev/null; then
  echo 'OpenSSH fixture could not start; see platform prerequisites (no existing server was modified).' >&2
  cat "$fixture_dir/sshd.log" >&2
  exit 1
fi
mkdir -p "$fixture_dir/remote/nested" "$fixture_dir/remote/no-access"
python3 - "$fixture_dir" <<'PY_FIXTURE'
import os,sys
root=os.fsencode(sys.argv[1]); remote=root+b'/remote'
with open(remote+b'/hello.txt','wb') as f:f.write(b'\xef\xbb\xbfhello\r\nworld\r\n')
os.chmod(remote+b'/hello.txt',0o640)
try:
    with open(remote+b'/bad_\xff.txt','wb') as f:f.write(b'non-utf8 name\n')
except OSError:
    with open(root+b'/nonutf8-unavailable','wb') as f:f.write(b'filesystem does not accept non-UTF8 names')
with open(remote+b'/binary.bin','wb') as f:f.write(b'\x00\xffbinary')
with open(remote+b'/nested/inside.txt','wb') as f:f.write(b'nested\n')
with open(root+b'/outside.txt','wb') as f:f.write(b'NEVER_FOLLOW_THIS_LINK')
os.symlink(root+b'/outside.txt',remote+b'/link-outside')
with open(remote+b'/large.bin','wb') as f:
    for _ in range(128):f.write(b'L'*65536)
os.chmod(remote+b'/no-access',0)
PY_FIXTURE
export UNNY_TEST_SSH_PORT="$fixture_port" UNNY_TEST_SSH_USER="$fixture_user" UNNY_TEST_SSH_DIR="$fixture_dir"
if [ "${1:-}" = '--desktop' ]; then
  export UNNY_SMOKE_DATA_DIR="$fixture_dir/vault"
  cargo test --manifest-path src-tauri/Cargo.toml services::sessions::tests::native_seed -- --ignored --exact
  src-tauri/target/debug/bundle/macos/unny-ssh.app/Contents/MacOS/unny-ssh
elif [ "${1:-}" = '--files' ]; then
  cargo test --manifest-path src-tauri/Cargo.toml services::sessions::tests::native_files -- --ignored --exact --nocapture
else
  cargo test --manifest-path src-tauri/Cargo.toml services::sessions::tests::native_openssh -- --ignored --exact --nocapture
fi
