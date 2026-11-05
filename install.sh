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

# Hardware helpers are optional (AGENTS.md rule 5): only suggest the ones for hardware that is
# plugged in, and never install them.
has_usb_vendor() { # $1 = vendor ID, 4 hex digits
  if [ "$(uname -s)" = "Darwin" ]; then
    ioreg -p IOUSB -l -w0 2>/dev/null | grep -q "\"idVendor\" = $((16#$1))\b"
  else
    grep -qsi "^$1$" /sys/bus/usb/devices/*/idVendor
  fi
}
hardware_hints() {
  if has_usb_vendor 1220 && ! command -v goxlr-daemon >/dev/null 2>&1 \
    && [ ! -d "/Applications/GoXLR Utility.app" ]; then
    echo "🎚️ GoXLR detected. For GoXLR control, install GoXLR Utility:"
    if [ "$(uname -s)" = "Darwin" ]; then
      echo "   https://github.com/GoXLR-on-Linux/goxlr-utility/releases"
    else
      echo "   Arch: yay -S goxlr-utility · others: https://github.com/GoXLR-on-Linux/goxlr-utility"
    fi
  fi
  if has_usb_vendor 1038 && ! command -v headsetcontrol >/dev/null 2>&1; then
    echo "🎧 SteelSeries device detected. For headset presence and battery, install headsetcontrol:"
    echo "   https://github.com/Sapd/HeadsetControl (Arch: pacman -S headsetcontrol)"
  fi
}

# macOS: launchd agent instead of systemd/D-Bus/XDG desktop files.
if [ "$(uname -s)" = "Darwin" ]; then
  BIN_DIR="${HOME}/.local/bin"
  AGENT_DIR="${HOME}/Library/LaunchAgents"
  AGENT="${AGENT_DIR}/org.sonomux.sonomuxd.plist"
  DOMAIN="gui/$(id -u)"
  mkdir -p "${BIN_DIR}" "${AGENT_DIR}" "${HOME}/Library/Logs"

  # Stop before copying so the running binary is never overwritten.
  TRAY_AGENT="${AGENT_DIR}/org.sonomux.tray.plist"
  launchctl bootout "${DOMAIN}/org.sonomux.tray" 2>/dev/null || true
  launchctl bootout "${DOMAIN}/org.sonomux.sonomuxd" 2>/dev/null || true
  # bootout returns before the job is gone; bootstrap fails (error 5) if it is still loaded.
  for _ in $(seq 1 50); do
    launchctl print "${DOMAIN}/org.sonomux.sonomuxd" >/dev/null 2>&1 \
      || launchctl print "${DOMAIN}/org.sonomux.tray" >/dev/null 2>&1 || break
    sleep 0.1
  done

  echo "🚀 Installing binaries to ${BIN_DIR}..."
  install -m 755 target/release/sonomux target/release/sonomuxd target/release/sonomuxctl \
    target/release/sonomux-tray "${BIN_DIR}/"

  echo "⚙️ Configuring launchd agent..."
  sed -e "s|@BIN_DIR@|${BIN_DIR}|g" -e "s|@LOG_FILE@|${HOME}/Library/Logs/sonomuxd.log|g" \
    dist/launchd/org.sonomux.sonomuxd.plist > "${AGENT}"
  launchctl bootstrap "${DOMAIN}" "${AGENT}"
  sed -e "s|@BIN_DIR@|${BIN_DIR}|g" -e "s|@LOG_FILE@|${HOME}/Library/Logs/sonomux-tray.log|g" \
    dist/launchd/org.sonomux.tray.plist > "${TRAY_AGENT}"
  launchctl bootstrap "${DOMAIN}" "${TRAY_AGENT}"

  echo ""
  hardware_hints
  echo "✅ Sonomux installed and running (log: ~/Library/Logs/sonomuxd.log)."
  echo "   Run 'sonomuxctl status' or launch 'sonomux' GUI to verify."
  exit 0
fi

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
install -m 755 target/release/sonomux-tray "${BIN_DIR}/sonomux-tray"

# 5. Install systemd user service
echo "⚙️ Configuring systemd user service..."
sed "s|/usr/bin/sonomuxd|${BIN_DIR}/sonomuxd|g" dist/systemd/sonomuxd.service > "${SYSTEMD_DIR}/sonomuxd.service"
sed "s|/usr/bin/sonomux-tray|${BIN_DIR}/sonomux-tray|g" dist/systemd/sonomux-tray.service > "${SYSTEMD_DIR}/sonomux-tray.service"
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
systemctl --user enable --now sonomux-tray.service || echo "⚠️ No graphical session: sonomux-tray starts at next login."

echo ""
hardware_hints
echo "✅ Sonomux installed and running successfully!"
echo "   Run 'sonomuxctl status' or launch 'sonomux' GUI to verify."
