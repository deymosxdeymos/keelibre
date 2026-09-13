#!/usr/bin/env bash
# install keebyd for the current user
set -e
cd "$(dirname "$0")/.."
make
mkdir -p ~/.local/bin ~/.config/keebyd
cp bin/keebyd ~/.local/bin/
mkdir -p ~/.config/systemd/user
cp etc/keebyd.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now keebyd
echo
echo "keebyd installed and running."
echo "  switch picker UI : http://127.0.0.1:7777"
echo "  toggle mute      : Ctrl+K x3  (or systemctl --user restart keebyd)"
echo "  logs             : journalctl --user -u keebyd -f"
