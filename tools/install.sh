#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."
cargo build --release --locked
mkdir -p ~/.local/bin ~/.config/keebyd ~/.local/share/applications
install -m755 target/release/keebyd ~/.local/bin/keebyd
install -m644 etc/keebyd.desktop ~/.local/share/applications/keebyd.desktop
mkdir -p ~/.config/systemd/user
cp etc/keebyd.service ~/.config/systemd/user/
cp etc/keebyd-ui.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable keebyd
systemctl --user enable keebyd-ui
systemctl --user restart keebyd
systemctl --user restart keebyd-ui
echo
echo "keebyd installed and running."
echo "  control panel     : launch Keebyd from your application menu"
echo "  toggle mute      : use the Keebyd tray menu"
echo "  logs             : journalctl --user -u keebyd -f"
