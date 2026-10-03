#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."
cargo build --release --locked
mkdir -p ~/.local/bin ~/.config/keebyd ~/.local/share/applications
install -m755 target/release/keebyd ~/.local/bin/keebyd
sound_dir="${XDG_DATA_HOME:-$HOME/.local/share}/keebyd/sounds"
mkdir -p "$sound_dir"
# Install bundled synthetic packs, but never replace a user's customized file.
while IFS= read -r -d '' source; do
    relative="${source#packs/}"
    destination="$sound_dir/$relative"
    if [ ! -e "$destination" ]; then
        mkdir -p "$(dirname "$destination")"
        install -m644 "$source" "$destination"
    fi
done < <(find packs -type f -print0)
test -f "$sound_dir/thocky-linear/profile.conf"
install -m644 packaging/linux/keebyd.desktop ~/.local/share/applications/keebyd.desktop
mkdir -p ~/.config/systemd/user
cp packaging/linux/keebyd.service ~/.config/systemd/user/
cp packaging/linux/keebyd-ui.service ~/.config/systemd/user/
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
