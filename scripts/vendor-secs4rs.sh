#!/usr/bin/env bash
# Re-package secs4rs and vendor it into src-tauri/vendor (NuGet/DLL-style pin).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SECS4RS_ROOT="${1:-$ROOT/../secs4rs}"
VENDOR_DIR="$ROOT/src-tauri/vendor"

if [[ ! -f "$SECS4RS_ROOT/Cargo.toml" ]]; then
  echo "secs4rs workspace not found: $SECS4RS_ROOT" >&2
  echo "Usage: $0 [/path/to/secs4rs]" >&2
  exit 1
fi

echo "Packaging secs4rs from: $SECS4RS_ROOT"
(
  cd "$SECS4RS_ROOT"
  cargo package -p secs4rs --allow-dirty --no-verify
)

# Resolve version from packaged crate name
PKG_DIR="$SECS4RS_ROOT/target/package"
CRATE_FILE="$(ls -1 "$PKG_DIR"/secs4rs-*.crate | sort -V | tail -1)"
BASENAME="$(basename "$CRATE_FILE" .crate)"   # secs4rs-0.1.0
VERSION="${BASENAME#secs4rs-}"

echo "Vendoring $BASENAME -> $VENDOR_DIR"
rm -rf "$VENDOR_DIR/secs4rs" "$VENDOR_DIR"/secs4rs-*.crate
mkdir -p "$VENDOR_DIR"
cp "$CRATE_FILE" "$VENDOR_DIR/"
tar -xzf "$CRATE_FILE" -C "$VENDOR_DIR"
rm -rf "$VENDOR_DIR/secs4rs"
mv "$VENDOR_DIR/$BASENAME" "$VENDOR_DIR/secs4rs"
rm -f "$VENDOR_DIR/secs4rs/Cargo.toml.orig"

# Refresh vendor README table (fixed structure)
cat > "$VENDOR_DIR/README.md" << MD
# Vendored packages

Third-party Rust crates packaged into this app (C# NuGet / DLL style).

| Crate | Version | Artifact |
|-------|---------|----------|
| secs4rs | ${VERSION} | \`${BASENAME}.crate\` |

- \`secs4rs/\` — extracted package used by Cargo (\`path\` dependency)
- \`${BASENAME}.crate\` — immutable package archive from \`cargo package\`

Upgrade:

\`\`\`bash
# from simulator repo root
./scripts/vendor-secs4rs.sh /path/to/secs4rs
\`\`\`
MD

# Keep Cargo.toml version pin in sync if present
CARGO_TOML="$ROOT/src-tauri/Cargo.toml"
if grep -q 'secs4rs = {' "$CARGO_TOML"; then
  # BSD/macOS sed
  sed -i '' -E "s|secs4rs = \\{ path = \"vendor/secs4rs\", version = \"[^\"]+\" \\}|secs4rs = { path = \"vendor/secs4rs\", version = \"${VERSION}\" }|" "$CARGO_TOML" || true
fi

echo "Done. secs4rs ${VERSION} vendored."
echo "Next: cd src-tauri && cargo check"
