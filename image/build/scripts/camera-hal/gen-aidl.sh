#!/bin/sh
# NDK sources for the camera AIDL interfaces the provider needs, from their
# frozen versions, as Soong's aidl_interface would generate them.
set -eu
T=$(readlink -f "${1:?usage: gen-aidl.sh <lineage tree> <work dir>}")
H=$T/hardware/interfaces
AIDL=$T/out/host/linux_musl-arm64/bin/aidl
O=$(readlink -f "${2:?}")/gen
API_COMMON=$H/camera/common/aidl/aidl_api/android.hardware.camera.common/1
API_META=$H/camera/metadata/aidl/aidl_api/android.hardware.camera.metadata/1
API_DEVICE=$H/camera/device/aidl/aidl_api/android.hardware.camera.device/1
API_PROVIDER=$H/camera/provider/aidl/aidl_api/android.hardware.camera.provider/1
API_HCOMMON=$H/common/aidl/aidl_api/android.hardware.common/2
API_FMQ=$H/common/fmq/aidl/aidl_api/android.hardware.common.fmq/1
API_GRAPHICS=$H/graphics/common/aidl/aidl_api/android.hardware.graphics.common/current
INC="-I$API_COMMON -I$API_META -I$API_DEVICE -I$API_PROVIDER -I$API_HCOMMON -I$API_FMQ -I$API_GRAPHICS"
rm -rf "$O"; mkdir -p "$O/include" "$O/src"
gen() { # api-dir version only-headers
	dir=$1 version=$2 headers=$3
	hash=$(tail -1 "$dir/.hash" 2>/dev/null || true)
	find "$dir" -name '*.aidl' | sort | while read -r f; do
		out=$O/src; [ "$headers" = yes ] && out=$O/hdronly
		mkdir -p "$out"
		"$AIDL" --lang=ndk --structured --stability=vintf --min_sdk_version=current \
			--version="$version" ${hash:+--hash=$hash} $INC -h "$O/include" -o "$out" "$f"
	done
}
gen "$API_COMMON" 1 no
gen "$API_META" 1 no
gen "$API_DEVICE" 1 no
gen "$API_PROVIDER" 1 no
gen "$API_HCOMMON" 2 yes
gen "$API_FMQ" 1 yes
gen "$API_GRAPHICS" 7 yes
rm -rf "$O/hdronly"
find "$O/src" -name '*.cpp' | wc -l
