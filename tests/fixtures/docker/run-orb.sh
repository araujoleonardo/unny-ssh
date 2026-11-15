#!/usr/bin/env bash
set -euo pipefail
# This script NEVER invokes the host's Docker CLI or reuses an existing machine.
command -v orb >/dev/null
fixture_dir=$(mktemp -d)
fixture_suffix=$(basename "$fixture_dir" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9')
fixture_machine="unny-qa-${fixture_suffix}"
fixture_bridge=""
fixture_created=0
cleanup(){
  if [ -n "$fixture_bridge" ]; then kill "$fixture_bridge" 2>/dev/null || true; fi
  if [ "$fixture_created" = '1' ]; then orb delete --force "$fixture_machine"; fi
  chmod -R u+rwX "$fixture_dir" 2>/dev/null || true
  rm -rf "$fixture_dir"
}
trap cleanup EXIT INT TERM
chmod 700 "$fixture_dir"
ssh-keygen -q -t ed25519 -N '' -f "$fixture_dir/id_ed25519"
orb create --isolated --memory 3G --cpus 2 --disk 12G ubuntu:24.04 "$fixture_machine"
fixture_created=1
orb -m "$fixture_machine" -u root bash -lc 'apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq docker.io docker-compose-v2 openssh-server netcat-openbsd && systemctl start docker'
orb -m "$fixture_machine" -u root bash -c 'cat > /opt/unny-setup.sh' < tests/fixtures/docker/setup-vm.sh
orb -m "$fixture_machine" -u root env UNNY_DISPOSABLE_DOCKER=1 bash /opt/unny-setup.sh < "$fixture_dir/id_ed25519.pub"
orb -m "$fixture_machine" -u root cat /opt/unny-fixture/host.pub > "$fixture_dir/host.pub"
python3 tests/fixtures/docker/bridge.py "$fixture_machine" "$fixture_dir/port" &
fixture_bridge=$!
for _ in $(seq 1 100); do [ -f "$fixture_dir/port" ] && break; sleep 0.1; done
export UNNY_DOCKER_FIXTURE_DIR="$fixture_dir" UNNY_DOCKER_FIXTURE_HOST=127.0.0.1 UNNY_DOCKER_FIXTURE_PORT
UNNY_DOCKER_FIXTURE_PORT=$(cat "$fixture_dir/port")
if [ "${1:-}" = '--desktop' ]; then
  export UNNY_SMOKE_DATA_DIR="$fixture_dir/ui-vault"
  cargo test --manifest-path src-tauri/Cargo.toml services::sessions::docker_tests::native_docker_seed -- --ignored --exact
  src-tauri/target/debug/bundle/macos/unny-ssh.app/Contents/MacOS/unny-ssh
else
  cargo test --manifest-path src-tauri/Cargo.toml services::sessions::docker_tests::native_docker_compose_lifecycle -- --ignored --exact
  orb -m "$fixture_machine" -u root docker run -d --name unny-container-test --mount type=volume,source=unny-in-use,target=/data busybox:1.37.0 sh -c 'echo container-ready; while :; do sleep 2; done'
  cargo test --manifest-path src-tauri/Cargo.toml services::sessions::docker_tests::native_lock_cancels_channels -- --ignored --exact
  cargo test --manifest-path src-tauri/Cargo.toml services::sessions::docker_tests::native_docker_missing -- --ignored --exact
  orb -m "$fixture_machine" -u root mv /usr/libexec/docker/cli-plugins/docker-compose /usr/libexec/docker/cli-plugins/docker-compose.disabled
  cargo test --manifest-path src-tauri/Cargo.toml services::sessions::docker_tests::native_compose_missing -- --ignored --exact
fi
