#!/bin/sh
set -eu

cargo_auditable_version="0.7.4"
archive="cargo-auditable-x86_64-unknown-linux-musl.tar.xz"
digest="4a4f0c124543c065f03d89aee26550305143c6e4af3e46270dbabefeb79895d2"
url="https://github.com/rust-secure-code/cargo-auditable/releases/download/v${cargo_auditable_version}/${archive}"
tmpdir="$(mktemp -d)"
trap 'rm -rf -- "$tmpdir"' EXIT HUP INT TERM

apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates curl xz-utils
curl -fsSL --retry 3 --output "${tmpdir}/${archive}" "$url"
printf '%s  %s\n' "$digest" "${tmpdir}/${archive}" | sha256sum -c -
tar -xJf "${tmpdir}/${archive}" -C "$tmpdir"
install -m 0755 \
    "${tmpdir}/cargo-auditable-x86_64-unknown-linux-musl/cargo-auditable" \
    /usr/local/bin/cargo-auditable

# Cross mounts the toolchain at /rust only when the image runs.  The shim is
# created while the custom image is built and deliberately resolves the
# pinned cargo-auditable binary from the same immutable image.
printf '%s\n' '#!/bin/sh' 'exec /rust/bin/cargo auditable "$@"' \
    > /usr/local/bin/cargo
chmod 0755 /usr/local/bin/cargo

rm -rf /var/lib/apt/lists/*
