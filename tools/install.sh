#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."
cargo build --release --locked
mkdir -p ~/.local/bin ~/.config/keebyd ~/.local/share/applications
install -m755 target/release/keebyd ~/.local/bin/keebyd
install -m644 etc/keebyd.desktop ~/.local/share/applications/keebyd.desktop
mkdir -p ~/.config/systemd/user
cp etc/keebyd.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now keebyd
echo
echo "keebyd installed and running."
echo "  control panel     : launch Keebyd from your application menu"
echo "  toggle mute      : Ctrl+K x3  (or systemctl --user restart keebyd)"
echo "  logs             : journalctl --user -u keebyd -f"
