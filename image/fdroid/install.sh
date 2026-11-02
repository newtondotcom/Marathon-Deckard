#!/bin/sh
# Lays out F-Droid, its Privileged Extension and Aurora Store from f-droid.org
# under an overlay root, as F-Droid's own OTA does: install.sh <F-Droid apk>
# <Privileged Extension apk> <Aurora Store apk> <overlay root> <abi>. The APKs
# keep their f-droid.org signatures so F-Droid can update all three; their
# native code for <abi> is unpacked beside them.
set -eu
fdroid=$1 privext=$2 aurora=$3 root=$4 abi=$5
case "$abi" in
arm64-v8a) isa=arm64 ;;
x86_64) isa=x86_64 ;;
*) echo "install.sh: unknown ABI $abi" >&2; exit 2 ;;
esac
here=$(cd "$(dirname "$0")" && pwd)
pinned() {
	want=$(awk -v f="$(basename "$1")" '$2 == f { print $1 }' "$here/SHA256SUMS")
	[ -n "$want" ] && [ "$(sha256sum "$1" | cut -d ' ' -f 1)" = "$want" ] ||
		{ echo "install.sh: $1 is not a pinned f-droid.org release" >&2; exit 1; }
}
pinned "$fdroid"
pinned "$privext"
pinned "$aurora"
place() {
	rm -rf "$root/system/$1"
	install -Dm644 "$2" "$root/system/$1/$(basename "$1").apk"
	if unzip -Z1 "$2" "lib/$abi/*.so" >/dev/null 2>&1; then
		mkdir -p "$root/system/$1/lib/$isa"
		unzip -q -j -o "$2" "lib/$abi/*.so" -d "$root/system/$1/lib/$isa"
		chmod 644 "$root/system/$1/lib/$isa"/*.so
	fi
}
place app/F-Droid "$fdroid"
place priv-app/F-DroidPrivilegedExtension "$privext"
place app/AuroraStore "$aurora"
