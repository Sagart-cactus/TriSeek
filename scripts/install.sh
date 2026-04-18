#!/bin/sh

set -eu

DEFAULT_REPO="Sagart-cactus/TriSeek"
DEFAULT_INSTALL_DIR="${HOME}/.local/bin"

usage() {
  cat <<'EOF'
Install TriSeek from GitHub Releases.

Usage:
  install.sh [--version <tag>] [--install-dir <dir>] [--repo <owner/name>]

Examples:
  install.sh
  install.sh --version v0.2.1
  install.sh --install-dir /usr/local/bin

Environment overrides:
  TRISEEK_VERSION
  TRISEEK_INSTALL_DIR
  TRISEEK_REPO
EOF
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

resolve_path() {
  case "$1" in
    /*)
      printf '%s\n' "$1"
      ;;
    *)
      printf '%s/%s\n' "$(pwd)" "$1"
      ;;
  esac
}

triseek_home_dir() {
  if [ -n "${TRISEEK_HOME:-}" ]; then
    resolve_path "$TRISEEK_HOME"
    return
  fi
  printf '%s\n' "${HOME}/.triseek"
}

daemon_dir() {
  printf '%s/daemon\n' "$(triseek_home_dir)"
}

daemon_pid_file() {
  printf '%s/%s\n' "$(daemon_dir)" "daemon.pid"
}

daemon_port_file() {
  printf '%s/%s\n' "$(daemon_dir)" "daemon.port"
}

cleanup_stale_daemon_files() {
  rm -f "$(daemon_pid_file)" "$(daemon_port_file)"
}

read_daemon_pid() {
  pid_file="$(daemon_pid_file)"
  [ -f "$pid_file" ] || return 1
  pid="$(tr -d '[:space:]' < "$pid_file" 2>/dev/null || true)"
  [ -n "$pid" ] || return 1
  printf '%s\n' "$pid"
}

daemon_pid_running() {
  kill -0 "$1" 2>/dev/null
}

wait_for_daemon_exit() {
  attempts=0
  while [ "$attempts" -lt 50 ]; do
    pid="$(read_daemon_pid)" || return 0
    if ! daemon_pid_running "$pid"; then
      cleanup_stale_daemon_files
      return 0
    fi
    sleep 0.2
    attempts=$((attempts + 1))
  done
  return 1
}

stop_existing_daemon() {
  pid="$(read_daemon_pid)" || {
    cleanup_stale_daemon_files
    return 0
  }

  if [ -x "$install_path" ]; then
    "$install_path" daemon stop >/dev/null 2>&1 || true
  elif have_cmd triseek; then
    triseek daemon stop >/dev/null 2>&1 || true
  fi

  if wait_for_daemon_exit; then
    return 0
  fi

  if daemon_pid_running "$pid"; then
    kill "$pid" 2>/dev/null || true
  fi

  if wait_for_daemon_exit; then
    return 0
  fi

  if daemon_pid_running "$pid"; then
    kill -9 "$pid" 2>/dev/null || true
  fi

  wait_for_daemon_exit || die "failed to stop existing TriSeek daemon (pid $pid)"
}

ensure_daemon_running() {
  action="$1"
  printf '%s TriSeek daemon...\n' "$action"
  if ! "$install_path" daemon start; then
    die "failed to start TriSeek daemon after install"
  fi
}

normalize_version() {
  case "$1" in
    "" | latest)
      printf 'latest'
      ;;
    v*)
      printf '%s' "$1"
      ;;
    *)
      printf 'v%s' "$1"
      ;;
  esac
}

download_file() {
  url="$1"
  destination="$2"

  if have_cmd curl; then
    if curl --fail --silent --show-error --location "$url" --output "$destination"; then
      return 0
    fi
    return 1
  fi

  if have_cmd wget; then
    if wget --quiet --output-document="$destination" "$url"; then
      return 0
    fi
    return 1
  fi

  die "curl or wget is required to download TriSeek"
}

cargo_install_from_git() {
  cargo_root="${tmpdir}/cargo-root"

  have_cmd cargo || die "failed to download a release archive and cargo is not installed for source fallback"

  printf 'No matching GitHub Release archive was found. Falling back to cargo install.\n' >&2
  mkdir -p "$cargo_root"

  if [ "$version" = "latest" ]; then
    cargo install \
      --locked \
      --root "$cargo_root" \
      --git "https://github.com/${repo}.git" \
      triseek
    cargo install \
      --locked \
      --root "$cargo_root" \
      --git "https://github.com/${repo}.git" \
      search-server
  else
    cargo install \
      --locked \
      --root "$cargo_root" \
      --git "https://github.com/${repo}.git" \
      --tag "$version" \
      triseek
    cargo install \
      --locked \
      --root "$cargo_root" \
      --git "https://github.com/${repo}.git" \
      --tag "$version" \
      search-server
  fi

  printf '%s\n' "$cargo_root"
}

version="${TRISEEK_VERSION:-latest}"
install_dir="${TRISEEK_INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
repo="${TRISEEK_REPO:-$DEFAULT_REPO}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ "$#" -ge 2 ] || die "--version requires a value"
      version="$2"
      shift 2
      ;;
    --install-dir)
      [ "$#" -ge 2 ] || die "--install-dir requires a value"
      install_dir="$2"
      shift 2
      ;;
    --repo)
      [ "$#" -ge 2 ] || die "--repo requires a value"
      repo="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

install_path="${install_dir}/triseek"
server_install_path="${install_dir}/triseek-server"

have_cmd tar || die "tar is required to extract TriSeek"
have_cmd find || die "find is required to locate the installed binary"

os_name="$(uname -s)"
arch_name="$(uname -m)"

case "$os_name" in
  Darwin)
    os="macos"
    archive_ext="tar.gz"
    ;;
  Linux)
    os="linux"
    archive_ext="tar.gz"
    ;;
  *)
    die "unsupported operating system: $os_name"
    ;;
esac

case "$arch_name" in
  x86_64|amd64)
    arch="x86_64"
    ;;
  arm64|aarch64)
    case "$os" in
      macos)
        arch="aarch64"
        ;;
      *)
        die "prebuilt $os releases are not published for $arch_name"
        ;;
    esac
    ;;
  *)
    die "unsupported architecture: $arch_name"
    ;;
esac

version="$(normalize_version "$version")"
asset_stub="${os}-${arch}"

if [ "$version" = "latest" ]; then
  archive_name="triseek-${asset_stub}.${archive_ext}"
  download_url="https://github.com/${repo}/releases/latest/download/${archive_name}"
else
  archive_name="triseek-${version}-${asset_stub}.${archive_ext}"
  download_url="https://github.com/${repo}/releases/download/${version}/${archive_name}"
fi

tmpdir="$(mktemp -d 2>/dev/null || mktemp -d -t triseek-install)"
archive_path="${tmpdir}/${archive_name}"
staging_dir="${tmpdir}/extract"

cleanup() {
  rm -rf "$tmpdir"
}

trap cleanup EXIT INT TERM HUP

mkdir -p "$staging_dir" "$install_dir"

printf 'Downloading %s\n' "$download_url"
if download_file "$download_url" "$archive_path"; then
  tar -xzf "$archive_path" -C "$staging_dir"
  binary_path="$(find "$staging_dir" -type f -name triseek | head -n 1)"
  server_binary_path="$(find "$staging_dir" -type f -name triseek-server | head -n 1)"
  [ -n "$binary_path" ] || die "downloaded archive did not contain a triseek binary"
  [ -n "$server_binary_path" ] || die "downloaded archive did not contain a triseek-server binary"
else
  install_root="$(cargo_install_from_git)"
  binary_path="${install_root}/bin/triseek"
  server_binary_path="${install_root}/bin/triseek-server"
fi

had_existing_install=0
if [ -e "$install_path" ] || [ -e "$server_install_path" ]; then
  had_existing_install=1
fi

daemon_was_running=0
existing_daemon_pid=""
if existing_daemon_pid="$(read_daemon_pid)"; then
  if daemon_pid_running "$existing_daemon_pid"; then
    daemon_was_running=1
  else
    cleanup_stale_daemon_files
  fi
fi

if [ "$daemon_was_running" -eq 1 ]; then
  printf 'Stopping existing TriSeek daemon (pid %s)...\n' "$existing_daemon_pid"
  stop_existing_daemon
fi

cp "$binary_path" "$install_path"
chmod 755 "$install_path"
cp "$server_binary_path" "$server_install_path"
chmod 755 "$server_install_path"

if ! "$install_path" help >/dev/null 2>&1; then
  die "installed binary did not pass the smoke check"
fi
if ! "$server_install_path" --help >/dev/null 2>&1; then
  die "installed daemon binary did not pass the smoke check"
fi

if [ "$had_existing_install" -eq 1 ] || [ "$daemon_was_running" -eq 1 ]; then
  ensure_daemon_running "Restarting"
else
  ensure_daemon_running "Starting"
fi

printf 'Installed triseek to %s\n' "$install_path"
printf 'Installed triseek-server to %s\n' "$server_install_path"

case ":$PATH:" in
  *":${install_dir}:"*)
    printf 'triseek is already on PATH\n'
    ;;
  *)
    printf 'Add it to PATH:\n'
    printf '  export PATH="%s:$PATH"\n' "$install_dir"
    ;;
esac

printf 'Try: triseek help\n'
