#!/usr/bin/env bash
# Install anneal — convergence assistant for knowledge corpora
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash
#
# Installs to ~/.local/bin by default. Set INSTALL_DIR to override:
#   curl -fsSL ... | INSTALL_DIR=/usr/local/bin bash

set -euo pipefail

REPO="flowerornament/anneal"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
SUPPORTED_RELEASE_TARGETS=(
    "aarch64-apple-darwin"
    "x86_64-unknown-linux-gnu"
    "aarch64-unknown-linux-gnu"
)

info()  { printf '\033[1;34m%s\033[0m\n' "$*"; }
error() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

source_install_hint() {
    cat >&2 <<'EOF'
Install from source:
  git clone https://github.com/flowerornament/anneal.git
  cargo install --path anneal --locked
EOF
    exit 1
}

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin) os="apple-darwin" ;;
    Linux)  os="unknown-linux-gnu" ;;
    *)      error "Unsupported OS: $OS" ;;
esac

case "$ARCH" in
    x86_64)  arch="x86_64" ;;
    aarch64|arm64) arch="aarch64" ;;
    *)       error "Unsupported architecture: $ARCH" ;;
esac

TARGET="${arch}-${os}"

supported=false
for supported_target in "${SUPPORTED_RELEASE_TARGETS[@]}"; do
    if [ "$TARGET" = "$supported_target" ]; then
        supported=true
        break
    fi
done

if [ "$supported" != true ]; then
    error "No prebuilt binary is published for $TARGET."
    source_install_hint
fi

# Get latest release tag
info "Finding latest release..."
TAG=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | head -1 | cut -d'"' -f4)

if [ -z "$TAG" ]; then
    error "No releases found."
    source_install_hint
fi

info "Installing anneal $TAG for $TARGET"

# Download and extract
URL="https://github.com/$REPO/releases/download/$TAG/anneal-$TARGET.tar.gz"
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

curl -fsSL "$URL" -o "$TMPDIR/anneal.tar.gz" || error "Download failed. Check that a binary exists for $TARGET"
tar xzf "$TMPDIR/anneal.tar.gz" -C "$TMPDIR"

# Install
mkdir -p "$INSTALL_DIR"
mv "$TMPDIR/anneal" "$INSTALL_DIR/anneal"
chmod +x "$INSTALL_DIR/anneal"

info "Installed to $INSTALL_DIR/anneal"

# Check PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
    echo ""
    echo "Add to your PATH:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
fi

echo ""
"$INSTALL_DIR/anneal" --version 2>/dev/null || true
info "Done. Run 'anneal status' in a knowledge corpus to get started."
