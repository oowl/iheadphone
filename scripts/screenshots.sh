#!/bin/sh
# Regenerate the README screenshots from preview mode.
#
# Each page is captured inside its own virtual KWin session with a private
# D-Bus session, so only the iHeadphone window can ever end up in an image.
# Requires KDE's kwin_wayland and spectacle.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/docs/screenshots"
cargo build --manifest-path "$root/Cargo.toml" -p iheadphone-desktop

capture() {
    file=$1
    shift
    dbus-run-session -- sh -c '
        out=$1; binary=$2; shift 2
        cfg=$(mktemp -d)
        socket=iheadphone-shot-$$
        env -u DISPLAY kwin_wayland --virtual --width 1260 --height 820 --socket "$socket" \
            --no-lockscreen --no-global-shortcuts --no-kactivities \
            --inputmethod /usr/bin/true >/dev/null 2>&1 &
        kwin=$!
        sleep 3
        env -u DISPLAY WAYLAND_DISPLAY="$socket" XDG_CONFIG_HOME="$cfg" IHEADPHONE_DEMO=1 "$@" \
            "$binary" >/dev/null 2>&1 &
        app=$!
        sleep 5
        env -u DISPLAY WAYLAND_DISPLAY="$socket" spectacle -b -n -a -e -S -o "$out" >/dev/null 2>&1
        kill "$app" "$kwin" 2>/dev/null || true
        wait 2>/dev/null || true
        rm -rf "$cfg"
    ' capture "$out/$file" "$root/target/debug/iheadphone" "$@" >/dev/null 2>&1
    echo "captured $file"
}

capture airpods-overview.png IHEADPHONE_OPEN_DEVICE=airpods IHEADPHONE_OPEN_PAGE=overview
capture airpods-sound.png IHEADPHONE_OPEN_DEVICE=airpods IHEADPHONE_OPEN_PAGE=sound
capture airpods-device.png IHEADPHONE_OPEN_DEVICE=airpods IHEADPHONE_OPEN_PAGE=device
capture sony-overview.png IHEADPHONE_OPEN_DEVICE=sony IHEADPHONE_OPEN_PAGE=overview
capture sony-sound.png IHEADPHONE_OPEN_DEVICE=sony IHEADPHONE_OPEN_PAGE=sound
