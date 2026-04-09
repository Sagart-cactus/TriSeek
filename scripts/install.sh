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
  install.sh --version v0.2.0
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

install_path="${install_dir}/triseek"
cp "$binary_path" "$install_path"
chmod 755 "$install_path"
cp "$server_binary_path" "${install_dir}/triseek-server"
chmod 755 "${install_dir}/triseek-server"

if ! "$install_path" help >/dev/null 2>&1; then
  die "installed binary did not pass the smoke check"
fi
if ! "${install_dir}/triseek-server" --help >/dev/null 2>&1; then
  die "installed daemon binary did not pass the smoke check"
fi

printf 'Installed triseek to %s\n' "$install_path"
printf 'Installed triseek-server to %s\n' "${install_dir}/triseek-server"

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
