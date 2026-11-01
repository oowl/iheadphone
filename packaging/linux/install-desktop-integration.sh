#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
project_root=$(CDPATH='' cd -- "$script_dir/../.." && pwd)
data_root=${XDG_DATA_HOME:-"$HOME/.local/share"}
app_id=io.github.iheadphone.iheadphone
desktop_path="$data_root/applications/$app_id.desktop"
icon_path="$data_root/icons/hicolor/512x512/apps/$app_id.png"
binary_path="$project_root/target/debug/iheadphone"

install -Dm644 \
    "$script_dir/$app_id.desktop" \
    "$desktop_path"
install -Dm644 \
    "$project_root/apps/desktop/assets/iheadphone-icon-512.png" \
    "$icon_path"

# Development runs are started through Cargo rather than an installed package.
# Use the resolved file here so Plasma does not retain a missing themed-icon
# placeholder; release packages can keep the portable icon name in the template.
if command -v desktop-file-edit >/dev/null 2>&1; then
    desktop-file-edit --set-icon="$icon_path" "$desktop_path"
    if [ -x "$binary_path" ]; then
        desktop-file-edit --set-key=Exec --set-value="$binary_path" "$desktop_path"
    fi
fi

if command -v kbuildsycoca6 >/dev/null 2>&1; then
    kbuildsycoca6 --noincremental
elif command -v kbuildsycoca5 >/dev/null 2>&1; then
    kbuildsycoca5 --noincremental
fi

printf 'Installed Wayland desktop integration for %s\n' "$app_id"
