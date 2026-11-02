#!/bin/sh
# Build an iHeadphone .app bundle and DMG from an existing macOS executable.
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
    echo "package-macos.sh must run on macOS" >&2
    exit 1
fi

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "usage: $0 <binary> <version> [output-directory]" >&2
    exit 1
fi

root=$(cd "$(dirname "$0")/.." && pwd)
binary=$1
version=$2
output=${3:-"$root/dist"}
build_number=${IHEADPHONE_BUILD_NUMBER:-0}
codesign_identity=${IHEADPHONE_CODESIGN_IDENTITY:--}

if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$'; then
    echo "version must be a semantic version without a leading v: $version" >&2
    exit 1
fi
if ! printf '%s\n' "$build_number" | grep -Eq '^[0-9]+([.][0-9]+){0,3}$'; then
    echo "IHEADPHONE_BUILD_NUMBER must contain one to four numeric components" >&2
    exit 1
fi
if [ ! -x "$binary" ]; then
    echo "macOS executable not found or not executable: $binary" >&2
    exit 1
fi

mkdir -p "$output"
output=$(cd "$output" && pwd)
app="$output/iHeadphone.app"
architectures=$(lipo -archs "$binary")
if printf '%s\n' "$architectures" | grep -qw arm64 \
    && printf '%s\n' "$architectures" | grep -qw x86_64; then
    package_arch=universal
elif printf '%s\n' "$architectures" | grep -qw arm64; then
    package_arch=arm64
elif printf '%s\n' "$architectures" | grep -qw x86_64; then
    package_arch=x86_64
else
    echo "unsupported macOS executable architecture: $architectures" >&2
    exit 1
fi
dmg="$output/iHeadphone-$version-$package_arch.dmg"
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT HUP INT TERM

rm -rf "$app"
rm -f "$dmg"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$binary" "$app/Contents/MacOS/iheadphone"
cp "$root/packaging/macos/Info.plist" "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $build_number" "$app/Contents/Info.plist"

icon_source="$root/apps/desktop/assets/iheadphone-icon.png"
cp "$icon_source" "$app/Contents/Resources/iheadphone-icon.png"

plutil -lint "$app/Contents/Info.plist" >/dev/null
xattr -cr "$app"
if [ "$codesign_identity" = "-" ]; then
    codesign --force --options runtime --sign - "$app"
else
    codesign --force --options runtime --timestamp --sign "$codesign_identity" "$app"
fi
codesign --verify --deep --strict --verbose=2 "$app"

staging="$temporary/dmg"
mkdir -p "$staging"
ditto "$app" "$staging/iHeadphone.app"
ln -s /Applications "$staging/Applications"
hdiutil create \
    -volname "iHeadphone $version" \
    -srcfolder "$staging" \
    -ov \
    -format UDZO \
    "$dmg"

if [ "$codesign_identity" != "-" ]; then
    codesign --force --timestamp --sign "$codesign_identity" "$dmg"
    codesign --verify --verbose=2 "$dmg"
fi

echo "$dmg"
