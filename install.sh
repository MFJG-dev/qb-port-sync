#!/usr/bin/env bash
set -euo pipefail

# install.sh - Install qb-port-sync binary, config, and systemd units

INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
CONFIG_DIR="${CONFIG_DIR:-/etc/qb-port-sync}"
SYSTEMD_SYSTEM_DIR="${SYSTEMD_SYSTEM_DIR:-/etc/systemd/system}"
SYSTEMD_USER_DIR="${HOME}/.config/systemd/user"

echo "==> Building qb-port-sync..."
cargo build --release --locked --all-features

echo "==> Installing binary to $INSTALL_DIR..."
sudo install -Dm755 target/release/qb-port-sync "$INSTALL_DIR/qb-port-sync"

echo "==> Creating config directory $CONFIG_DIR..."
sudo mkdir -p "$CONFIG_DIR"

if [ ! -f "$CONFIG_DIR/config.toml" ]; then
    echo "==> Installing example config to $CONFIG_DIR/config.example.toml..."
    sudo install -Dm644 config/config.example.toml "$CONFIG_DIR/config.example.toml"
    echo "    Please copy and edit $CONFIG_DIR/config.example.toml to $CONFIG_DIR/config.toml"
else
    echo "==> Config already exists at $CONFIG_DIR/config.toml, skipping..."
fi

# Detect systemd
if command -v systemctl &> /dev/null; then
    echo "==> Installing systemd service units..."
    
    # System-wide service
    if [ -d "$SYSTEMD_SYSTEM_DIR" ]; then
        sudo install -Dm644 systemd/qb-port-sync.service "$SYSTEMD_SYSTEM_DIR/qb-port-sync.service"
        echo "    Installed system service: $SYSTEMD_SYSTEM_DIR/qb-port-sync.service"
    fi
    
    # User-level path and oneshot service
    mkdir -p "$SYSTEMD_USER_DIR"
    install -Dm644 systemd/qb-port-sync.path "$SYSTEMD_USER_DIR/qb-port-sync.path"
    install -Dm644 systemd/qb-port-sync-oneshot.service "$SYSTEMD_USER_DIR/qb-port-sync-oneshot.service"
    echo "    Installed user units: $SYSTEMD_USER_DIR/qb-port-sync.{path,oneshot.service}"
    
    echo ""
    echo "To enable the system-wide service:"
    echo "  sudo systemctl daemon-reload"
    echo "  sudo systemctl enable --now qb-port-sync.service"
    echo ""
    echo "Or to enable the user-level file watcher:"
    echo "  systemctl --user daemon-reload"
    echo "  systemctl --user enable --now qb-port-sync.path"
else
    echo "==> systemd not detected, skipping service installation..."
fi

echo ""
echo "Installation complete!"
echo "Binary: $INSTALL_DIR/qb-port-sync"
echo "Config: $CONFIG_DIR/config.toml (edit as needed)"
