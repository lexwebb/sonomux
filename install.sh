#!/usr/bin/env bash
# ==============================================================================
# Sonomux Hands-Free User Installer
# Builds and installs Sonomux into ~/.local/bin without requiring root privileges.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

echo "🎛️ Installing Sonomux..."

# 1. Build release binaries
echo "📦 Building release workspace..."
cargo build --release --workspace

# 2. Ensure directories exist
BIN_DIR="${HOME}/.local/bin"
SYSTEMD_DIR="${HOME}/.config/systemd/user"
DBUS_DIR="${HOME}/.local/share/dbus-1/services"
DESKTOP_DIR="${HOME}/.local/share/applications"

mkdir -p "${BIN_DIR}" "${SYSTEMD_DIR}" "${DBUS_DIR}" "${DESKTOP_DIR}"

# 3. Stop running daemon if active
if systemctl --user is-active --quiet sonomuxd.service 2>/dev/null; then
  echo "⏸️ Stopping active sonomuxd.service..."
  systemctl --user stop sonomuxd.service
fi

# 4. Copy binaries
echo "🚀 Installing binaries to ${BIN_DIR}..."
install -m 755 target/release/sonomux "${BIN_DIR}/sonomux"
install -m 755 target/release/sonomuxd "${BIN_DIR}/sonomuxd"
install -m 755 target/release/sonomuxctl "${BIN_DIR}/sonomuxctl"

# 5. Install systemd user service
echo "⚙️ Configuring systemd user service..."
sed "s|/usr/bin/sonomuxd|${BIN_DIR}/sonomuxd|g" dist/systemd/sonomuxd.service > "${SYSTEMD_DIR}/sonomuxd.service"
systemctl --user daemon-reload

# 6. Install D-Bus auto-activation service
echo "📡 Configuring session D-Bus service..."
sed "s|/usr/bin/sonomuxd|${BIN_DIR}/sonomuxd|g" dist/dbus/org.sonomux.Daemon.service > "${DBUS_DIR}/org.sonomux.Daemon.service"

# 7. Install Desktop Entry
echo "🖥️ Installing desktop launcher..."
install -m 644 dist/desktop/org.sonomux.Sonomux.desktop "${DESKTOP_DIR}/org.sonomux.Sonomux.desktop"

# 8. Omarchy Bar Integration (if Omarchy is present)
if [ -d "${HOME}/.config/omarchy" ]; then
  OMARCHY_BAR_DIR="${HOME}/.config/omarchy/bar/scripts"
  mkdir -p "${OMARCHY_BAR_DIR}"
  install -m 755 dist/omarchy/sonomux-bar "${OMARCHY_BAR_DIR}/sonomux-bar"
  echo "✨ Omarchy bar script installed to ${OMARCHY_BAR_DIR}/sonomux-bar"
fi

# 9. Enable and start service
echo "▶️ Enabling and starting sonomuxd.service..."
systemctl --user enable --now sonomuxd.service

echo ""
echo "✅ Sonomux installed and running successfully!"
echo "   Run 'sonomuxctl status' or launch 'sonomux' GUI to verify."
