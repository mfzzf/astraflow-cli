#!/bin/sh
set -eu

REPOSITORY=${ASTF_REPOSITORY:-mfzzf/astraflow-cli}
VERSION=${ASTF_VERSION:-latest}

case "$(uname -s)" in
  Linux) os=unknown-linux-gnu ;;
  Darwin) os=apple-darwin ;;
  *)
    echo "astf: unsupported operating system: $(uname -s)" >&2
    echo "See https://github.com/${REPOSITORY}/releases for available binaries." >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  arm64 | aarch64) arch=aarch64 ;;
  *)
    echo "astf: unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

target="${arch}-${os}"
asset="astf-${target}.tar.gz"
if [ -n "${ASTF_RELEASE_BASE_URL:-}" ]; then
  base_url=${ASTF_RELEASE_BASE_URL%/}
elif [ "$VERSION" = latest ]; then
  base_url="https://github.com/${REPOSITORY}/releases/latest/download"
else
  case "$VERSION" in
    v*) tag=$VERSION ;;
    *) tag="v${VERSION}" ;;
  esac
  base_url="https://github.com/${REPOSITORY}/releases/download/${tag}"
fi

command -v curl >/dev/null 2>&1 || {
  echo "astf: curl is required" >&2
  exit 1
}
command -v tar >/dev/null 2>&1 || {
  echo "astf: tar is required" >&2
  exit 1
}

tmp_dir=$(mktemp -d 2>/dev/null || mktemp -d -t astf)
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

echo "Downloading astf for ${target}..."
curl -fsSL "${base_url}/${asset}" -o "${tmp_dir}/${asset}"
curl -fsSL "${base_url}/SHA256SUMS" -o "${tmp_dir}/SHA256SUMS"

expected=$(awk -v name="$asset" '$2 == name || $2 == "*" name { print $1; exit }' "${tmp_dir}/SHA256SUMS")
if [ -z "$expected" ]; then
  echo "astf: ${asset} is missing from SHA256SUMS" >&2
  exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "${tmp_dir}/${asset}" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "${tmp_dir}/${asset}" | awk '{print $1}')
else
  echo "astf: sha256sum or shasum is required" >&2
  exit 1
fi
if [ "$actual" != "$expected" ]; then
  echo "astf: checksum verification failed" >&2
  exit 1
fi

tar -xzf "${tmp_dir}/${asset}" -C "$tmp_dir"

if [ -n "${ASTF_INSTALL_DIR:-}" ]; then
  install_dir=$ASTF_INSTALL_DIR
elif [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
  install_dir=/usr/local/bin
else
  install_dir="${HOME:?HOME is required}/.local/bin"
fi
mkdir -p "$install_dir"

if command -v install >/dev/null 2>&1; then
  install -m 0755 "${tmp_dir}/astf" "${install_dir}/astf"
else
  cp "${tmp_dir}/astf" "${install_dir}/astf"
  chmod 0755 "${install_dir}/astf"
fi

echo "Installed astf to ${install_dir}/astf"
case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *) echo "Add ${install_dir} to PATH before running astf." ;;
esac
