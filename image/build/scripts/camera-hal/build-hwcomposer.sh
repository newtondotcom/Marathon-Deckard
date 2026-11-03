#!/bin/sh
# Builds Deckard's composer HAL (hwcomposer.deckard.so) outside Soong, with
# the flags Soong uses for a vendor arm64 library on this tree.
# usage: build-hwcomposer.sh <lineage tree> <work dir>
set -eu
T=$(readlink -f "$1"); W=$(readlink -f "$2"); shift 2
I=$T/out/soong/.intermediates
P=$T/out/target/product/deckard_arm64_only
CLANG=$T/prebuilts/clang/host/linux-arm64/clang-r596125
CXX=$CLANG/bin/clang++
CC=$CLANG/bin/clang
SRC=$T/hardware/deckard/hwcomposer
OBJ=$W/hwc-obj
mkdir -p "$OBJ" "$W/out/vendor/lib64/hw"
CFLAGS="-nostdlibinc -Werror=implicit-function-declaration -D__BIONIC_DEPRECATED_PAGE_SIZE_MACRO -O2 -Wall -Wextra
 -Wno-unused-parameter -Wno-c99-designator -Wno-gnu-folding-constant -Wno-inconsistent-missing-override
 -Wno-reorder-init-list -Wno-sign-compare -Wno-missing-field-initializers -Wno-unused
 -DANDROID -DNDEBUG -UDEBUG -D__compiler_offsetof=__builtin_offsetof -D__ANDROID_UNAVAILABLE_SYMBOLS_ARE_WEAK__
 -faddrsig -fcolor-diagnostics -ffp-contract=off -fno-exceptions -fno-strict-aliasing -no-canonical-prefixes
 -ftrivial-auto-var-init=zero -ffunction-sections -fdata-sections -fno-short-enums -funwind-tables
 -fstack-protector-strong -Wa,--noexecstack -D_FORTIFY_SOURCE=3 -march=armv8-a -target aarch64-linux-android10000
 -DANDROID_STRICT -fPIC -fvisibility-inlines-hidden -D__ANDROID_VNDK__ -D__ANDROID_VENDOR__ -D__ANDROID_VENDOR_API__=202504
 -D__LIBC_API__=10000 -D__LIBM_API__=10000 -D__LIBDL_API__=10000 -std=gnu++2b -fno-rtti"
INC="-I$SRC -I$I/hardware/deckard/hwcomposer/wayland_android_client_protocol_headers/gen
 -I$T/external/wayland/src -I$I/external/wayland/wayland_core_client_protocol_headers/gen
 -I$I/external/wayland-protocols/wayland_extension_client_protocol_headers/gen/wayland_protocol_codegen
 -I$T/external/libxkbcommon/include -I$T/external/libyuv/include -I$T/external/minigbm
 -I$T/external/libdrm/android -I$T/external/libdrm/include/drm -I$T/external/libdrm
 -I$T/system/core -I$T/system/core/libsync -I$T/system/core/libsync/include
 -I$T/frameworks/native/opengl/include -I$T/frameworks/native/libs/ui/include
 -I$T/frameworks/native/libs/ui/include_types -I$T/frameworks/native/libs/nativebase/include
 -I$T/frameworks/native/libs/arect/include -I$T/frameworks/native/libs/nativewindow/include
 -I$T/frameworks/native/libs/math/include -I$T/frameworks/native/libs/gralloc/types/include
 -I$T/frameworks/native/libs/binder/ndk/include_cpp -I$T/frameworks/native/libs/binder/ndk/include_ndk
 -I$T/frameworks/native/libs/binder/ndk/include_platform -I$T/frameworks/native/include
 -I$T/hardware/libhardware/include_all -I$T/hardware/libhardware/include
 -I$T/system/libbase/include -I$T/system/core/libutils/include -I$T/system/core/libutils/binder/include
 -I$T/system/core/libcutils/include_outside_system -I$T/system/logging/liblog/include_vndk
 -I$T/system/core/libsystem/include -I$T/system/libfmq/base -I$T/system/libhwbinder/include
 -I$T/system/libhidl/base/include -I$T/system/libhidl/transport/include
 -I$I/system/libhidl/transport/base/1.0/android.hidl.base@1.0_genc++_headers/gen
 -I$I/system/libhidl/transport/manager/1.0/android.hidl.manager@1.0_genc++_headers/gen
 -I$I/hardware/interfaces/graphics/common/1.0/android.hardware.graphics.common@1.0_genc++_headers/gen
 -I$I/hardware/interfaces/graphics/common/1.1/android.hardware.graphics.common@1.1_genc++_headers/gen
 -I$I/hardware/interfaces/graphics/common/1.2/android.hardware.graphics.common@1.2_genc++_headers/gen
 -I$I/hardware/interfaces/graphics/common/aidl/android.hardware.graphics.common-V7-ndk-source/gen/include
 -I$I/hardware/interfaces/graphics/composer/2.1/android.hardware.graphics.composer@2.1_genc++_headers/gen
 $(for h in clipboard/1.0/vendor.deckard.clipboard@1.0 display/1.0/vendor.deckard.display@1.0 display/1.1/vendor.deckard.display@1.1 display/1.2/vendor.deckard.display@1.2 display/1.3/vendor.deckard.display@1.3 task/1.0/vendor.deckard.task@1.0 window/1.0/vendor.deckard.window@1.0 window/1.1/vendor.deckard.window@1.1 window/1.2/vendor.deckard.window@1.2 window/1.3/vendor.deckard.window@1.3; do echo "-I$I/hardware/deckard/interfaces/${h}_genc++_headers/gen"; done)
 -I$CLANG/android_libc++/platform/aarch64/include/c++/v1 -I$CLANG/include/c++/v1
 -isystem $T/bionic/libc/include -isystem $T/bionic/libc/kernel/uapi/asm-arm64 -isystem $T/bionic/libc/kernel/uapi
 -isystem $T/bionic/libc/kernel/android/scsi -isystem $T/bionic/libc/kernel/android/uapi"
CFLAGS="$CFLAGS -DLOG_TAG=\"hwcomposer\" -Wall"
compile() {
	src=$1; o=$OBJ/$(echo "$src" | sed "s|$SRC/||; s|$I/||; s|/|_|g; s|\.c[p]*$|.o|")
	if [ ! -e "$o" ] || [ "$src" -nt "$o" ]; then
		echo "CC $src"
		case $src in
		*.c) nice -n 10 $CC -c $(echo "$CFLAGS" | sed "s/-std=gnu++2b//; s/-fno-rtti//; s/-fvisibility-inlines-hidden//") $INC -o "$o" "$src" ;;
		*) nice -n 10 $CXX -c $CFLAGS $INC -o "$o" "$src" ;;
		esac
	fi
}
if [ $# -gt 0 ]; then for f in "$@"; do compile "$f"; done; exit 0; fi
for f in $SRC/*.cpp $SRC/modes/*.cpp $I/hardware/deckard/hwcomposer/wayland_android_client_protocol_sources/gen/wayland-android-client-protocol.c; do echo "$f"; done | xargs -P 2 -I{} "$0" "$T" "$W" {}
V=$P/vendor/lib64; S=$P/system/lib64
OUT=$W/out/vendor/lib64/hw/hwcomposer.deckard.so
VLIBS="libutils libcutils libhardware libhidlbase libui libdrm libc++
 vendor.deckard.clipboard@1.0 vendor.deckard.display@1.0 vendor.deckard.display@1.1 vendor.deckard.display@1.2
 vendor.deckard.display@1.3 vendor.deckard.task@1.0 vendor.deckard.window@1.0 vendor.deckard.window@1.1
 vendor.deckard.window@1.2 vendor.deckard.window@1.3"
SLIBS="liblog libsync libEGL libGLESv2"
BIONIC="$I/bionic/libc/libc/android_vendor_arm64_armv8-a_shared/libc.so $I/bionic/libm/libm/android_vendor_arm64_armv8-a_shared/libm.so $I/bionic/libdl/libdl/android_vendor_arm64_armv8-a_shared/libdl.so"
echo "LD $OUT"
nice -n 10 $CXX -shared $I/bionic/libc/crtbegin_so/android_vendor_arm64_armv8-a/crtbegin_so.o $OBJ/*.o \
	$I/external/wayland/libwayland_client_static/android_vendor_arm64_armv8-a_static/libwayland_client_static.a \
	$I/external/wayland-protocols/libwayland_extension_client_protocols/android_vendor_arm64_armv8-a_static/libwayland_extension_client_protocols.a \
	$I/external/libxkbcommon/libxkbcommon/android_vendor_arm64_armv8-a_static/libxkbcommon.a \
	$I/external/libyuv/libyuv_static/android_vendor_arm64_armv8-a_static/libyuv_static.a \
	$I/external/libffi/libffi/android_vendor_arm64_armv8-a_static/libffi.a \
	$CLANG/android_libc++/platform/aarch64/lib/libc++demangle.a \
	$CLANG/lib/clang/22/lib/linux/libclang_rt.ubsan_minimal-aarch64-android.a \
	-Wl,--start-group $CLANG/lib/clang/22/lib/linux/libclang_rt.builtins-aarch64-android.a -Wl,--end-group \
	$(for l in $VLIBS; do echo "$V/$l.so"; done) $(for l in $SLIBS; do echo "$S/$l.so"; done) $BIONIC \
	$I/bionic/libc/crtend_so/android_vendor_arm64_armv8-a/crtend_so.o \
	-o "$OUT.unstripped" -target aarch64-linux-android10000 -Wl,-soname,hwcomposer.deckard.so \
	-Wl,-z,noexecstack -Wl,-z,relro -Wl,-z,now -Wl,--build-id=md5 -Wl,--no-undefined-version \
	-fuse-ld=lld -Wl,--icf=safe -Wl,--no-undefined -Wl,-z,separate-code -Wl,-z,separate-loadable-segments \
	-Wl,-z,max-page-size=4096 -Wl,--pack-dyn-relocs=android+relr -nostdlib -Bdynamic -Wl,--gc-sections \
	-Wl,--exclude-libs=libclang_rt.builtins-aarch64-android.a -Wl,--exclude-libs=libclang_rt.ubsan_minimal-aarch64-android.a
$CLANG/bin/llvm-strip --strip-all -o "$OUT" "$OUT.unstripped"
