#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "usage: $0 <iheadphone-binary> <version> [output-directory]" >&2
    exit 2
}

[[ $# -ge 2 && $# -le 3 ]] || usage

binary=$(realpath "$1")
version=$2
output=$(realpath -m "${3:-dist}")
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
project_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
desktop_file="$project_root/packaging/linux/io.github.iheadphone.iheadphone.desktop"
metainfo_file="$project_root/packaging/linux/io.github.iheadphone.iheadphone.metainfo.xml"
icon_file="$project_root/apps/desktop/assets/iheadphone-icon-512.png"
notices_file="$project_root/THIRD_PARTY_NOTICES.md"

[[ -x "$binary" ]] || { echo "executable not found: $binary" >&2; exit 1; }
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$ ]] \
    || { echo "version must look like 0.1.0: $version" >&2; exit 1; }
[[ $(uname -s) == Linux ]] || { echo "Linux packaging requires Linux" >&2; exit 1; }
[[ $(uname -m) == x86_64 ]] || { echo "only x86_64 Linux packages are supported" >&2; exit 1; }

for command in bsdtar dpkg-deb rpmbuild zstd; do
    command -v "$command" >/dev/null || { echo "missing packaging command: $command" >&2; exit 1; }
done

linuxdeploy=${LINUXDEPLOY:-}
[[ -n "$linuxdeploy" && -x "$linuxdeploy" ]] \
    || { echo "LINUXDEPLOY must point to an executable linuxdeploy AppImage" >&2; exit 1; }

if [[ -n ${SOURCE_DATE_EPOCH:-} ]]; then
    [[ $SOURCE_DATE_EPOCH =~ ^[0-9]+$ ]] \
        || { echo "SOURCE_DATE_EPOCH must be an integer timestamp" >&2; exit 1; }
    build_date=$SOURCE_DATE_EPOCH
else
    # Some packaging tools reject an exported-but-empty value. Keep local
    # packaging self-contained when no reproducible timestamp was supplied.
    unset SOURCE_DATE_EPOCH
    build_date=$(date +%s)
fi

mkdir -p "$output"
work=$(mktemp -d "${TMPDIR:-/tmp}/iheadphone-linux-package.XXXXXX")
trap 'rm -rf "$work"' EXIT

install_payload() {
    local root=$1
    install -Dm755 "$binary" "$root/usr/bin/iheadphone"
    install -Dm644 "$desktop_file" \
        "$root/usr/share/applications/io.github.iheadphone.iheadphone.desktop"
    install -Dm644 "$metainfo_file" \
        "$root/usr/share/metainfo/io.github.iheadphone.iheadphone.metainfo.xml"
    install -Dm644 "$icon_file" \
        "$root/usr/share/icons/hicolor/512x512/apps/io.github.iheadphone.iheadphone.png"
    install -Dm644 "$notices_file" \
        "$root/usr/share/doc/iheadphone/THIRD_PARTY_NOTICES.md"
}

# Debian/Ubuntu package. Optional helper programs are recommendations because
# the application still launches and provides reduced functionality without them.
deb_root="$work/deb"
install_payload "$deb_root"
mkdir -p "$deb_root/DEBIAN"
installed_size=$(du -sk "$deb_root/usr" | awk '{print $1}')
cat > "$deb_root/DEBIAN/control" <<EOF
Package: iheadphone
Version: $version
Section: sound
Priority: optional
Architecture: amd64
Installed-Size: $installed_size
Maintainer: iHeadphone contributors <noreply@github.com>
Homepage: https://github.com/oowl/iheadphone
Depends: libdbus-1-3, libfontconfig1, libvulkan1, libwayland-client0, libx11-xcb1, libxcb1, libxkbcommon0, libxkbcommon-x11-0
Recommends: bluez, libnotify-bin, playerctl, pulseaudio-utils
Description: Native desktop controller for AirPods and Sony headphones
 iHeadphone shows battery and connection state and provides sound, device,
 and listening-mode controls for supported headphones.
EOF
deb="$output/iheadphone_${version}_amd64.deb"
dpkg-deb --root-owner-group --build "$deb_root" "$deb"

# RPM package. rpmbuild discovers ELF library requirements from the staged
# binary; the explicit requirements cover helper programs invoked at runtime.
rpm_version=${version//-/~}
rpm_top="$work/rpmbuild"
mkdir -p "$rpm_top"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
rpm_payload="$work/rpm-payload"
install_payload "$rpm_payload"
tar -C "$rpm_payload" -czf "$rpm_top/SOURCES/iheadphone-$version.tar.gz" .
cat > "$rpm_top/SPECS/iheadphone.spec" <<EOF
Name:           iheadphone
Version:        $rpm_version
Release:        1
Summary:        Native desktop controller for AirPods and Sony headphones
License:        Apache-2.0
URL:            https://github.com/oowl/iheadphone
Source0:        iheadphone-$version.tar.gz
BuildArch:      x86_64
Requires:       bluez
Requires:       libnotify
Requires:       playerctl
Recommends:     pulseaudio-utils

%description
iHeadphone shows battery and connection state and provides sound, device,
and listening-mode controls for supported headphones.

%prep
%setup -q -c -T
tar -xzf %{SOURCE0}

%build

%install
mkdir -p %{buildroot}
cp -a usr %{buildroot}/

%files
/usr/bin/iheadphone
/usr/share/applications/io.github.iheadphone.iheadphone.desktop
/usr/share/metainfo/io.github.iheadphone.iheadphone.metainfo.xml
/usr/share/icons/hicolor/512x512/apps/io.github.iheadphone.iheadphone.png
/usr/share/doc/iheadphone/THIRD_PARTY_NOTICES.md

%changelog
* Mon Jan 01 2024 iHeadphone contributors <noreply@github.com> - $rpm_version-1
- Automated release package
EOF
rpmbuild -bb --define "_topdir $rpm_top" "$rpm_top/SPECS/iheadphone.spec"
rpm_source=$(find "$rpm_top/RPMS" -type f -name '*.rpm' -print -quit)
[[ -n "$rpm_source" ]] || { echo "rpmbuild did not produce a package" >&2; exit 1; }
rpm="$output/iheadphone-${version}-1.x86_64.rpm"
cp "$rpm_source" "$rpm"

# A binary Arch package made from the same Ubuntu 22.04 build. Arch carries a
# newer glibc, so this keeps one executable across every Linux package while
# retaining native pacman metadata.
arch_version=${version//-/_}
arch_root="$work/arch"
install_payload "$arch_root"
package_size=$(du -sb "$arch_root/usr" | awk '{print $1}')
cat > "$arch_root/.PKGINFO" <<EOF
pkgname = iheadphone
pkgbase = iheadphone
pkgver = $arch_version-1
pkgdesc = Native desktop controller for AirPods and Sony headphones
url = https://github.com/oowl/iheadphone
builddate = $build_date
packager = iHeadphone release workflow
size = $package_size
arch = x86_64
license = Apache-2.0
depend = bluez
depend = bluez-utils
depend = dbus
depend = fontconfig
depend = freetype2
depend = glibc
depend = libnotify
depend = libxcb
depend = libxkbcommon
depend = libxkbcommon-x11
depend = playerctl
depend = systemd-libs
depend = vulkan-icd-loader
depend = wayland
EOF
(
    cd "$arch_root"
    bsdtar -czf .MTREE --format=mtree \
        --options='!all,use-set,type,uid,gid,mode,time,size,md5,sha256,link' \
        .PKGINFO usr
)
arch="$output/iheadphone-${arch_version}-1-x86_64.pkg.tar.zst"
(
    cd "$arch_root"
    bsdtar --uid 0 --gid 0 --numeric-owner --zstd -cf "$arch" .PKGINFO .MTREE usr
)

# linuxdeploy creates the AppDir, discovers shared-library dependencies and
# invokes its bundled AppImage output plugin. Extract-and-run avoids requiring
# FUSE on GitHub-hosted runners.
appimage="$output/iHeadphone-$version-x86_64.AppImage"
appdir="$work/iHeadphone.AppDir"
install -Dm644 "$metainfo_file" \
    "$appdir/usr/share/metainfo/io.github.iheadphone.iheadphone.metainfo.xml"
NO_STRIP=1 \
LDAI_NO_APPSTREAM=1 \
LDAI_OUTPUT="$appimage" \
LINUXDEPLOY_OUTPUT_APP_NAME=iHeadphone \
LINUXDEPLOY_OUTPUT_VERSION="$version" \
    "$linuxdeploy" --appimage-extract-and-run \
        --appdir "$appdir" \
        --executable "$binary" \
        --desktop-file "$desktop_file" \
        --icon-file "$icon_file" \
        --icon-filename io.github.iheadphone.iheadphone \
        --output appimage

for artifact in "$deb" "$rpm" "$arch" "$appimage"; do
    [[ -s "$artifact" ]] || { echo "empty or missing artifact: $artifact" >&2; exit 1; }
    echo "$artifact"
done
