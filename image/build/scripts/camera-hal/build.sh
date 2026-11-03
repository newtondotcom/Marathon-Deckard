#!/bin/sh
# Builds android.hardware.camera.provider-V1-deckard-service outside Soong,
# with the flags Soong uses for a vendor arm64 binary on this tree.
# Used while a Soong re-analysis did not fit in the build host's memory.
# usage: build.sh <lineage tree> <work dir>   (after gen-aidl.sh with the same two)
#        build.sh <lineage tree> <work dir> <file.cpp>   (one object)
set -eu
T=$(readlink -f "$1"); W=$(readlink -f "$2"); shift 2
I=$T/out/soong/.intermediates
P=$T/out/target/product/deckard_arm64_only
CLANG=$T/prebuilts/clang/host/linux-arm64/clang-r596125
CXX=$CLANG/bin/clang++
SRC=$T/hardware/deckard/camera
OBJ=$W/obj
mkdir -p "$OBJ"
CFLAGS="-nostdlibinc -Werror=implicit-function-declaration -D__BIONIC_DEPRECATED_PAGE_SIZE_MACRO -O2 -Wall -Wextra
 -Wno-unused-parameter -Wno-c99-designator -Wno-gnu-folding-constant -Wno-inconsistent-missing-override
 -Wno-reorder-init-list -Wno-sign-compare -Wno-missing-field-initializers -Wno-unused
 -DANDROID -DNDEBUG -UDEBUG -D__compiler_offsetof=__builtin_offsetof -D__ANDROID_UNAVAILABLE_SYMBOLS_ARE_WEAK__
 -faddrsig -fcolor-diagnostics -ffp-contract=off -fno-exceptions -fno-strict-aliasing -no-canonical-prefixes
 -ftrivial-auto-var-init=zero -ffunction-sections -fdata-sections -fno-short-enums -funwind-tables
 -fstack-protector-strong -Wa,--noexecstack -D_FORTIFY_SOURCE=3 -march=armv8-a -target aarch64-linux-android10000
 -DANDROID_STRICT -fPIC -fvisibility-inlines-hidden -D__ANDROID_VNDK__ -D__ANDROID_VENDOR__ -D__ANDROID_VENDOR_API__=202504
 -D__LIBC_API__=10000 -D__LIBM_API__=10000 -D__LIBDL_API__=10000 -std=gnu++2b -fno-rtti"
INC="-I$SRC -I$W/gen/include
 -I$T/hardware/interfaces/camera/common/default/include -I$T/system/media/private/camera/include
 -I$T/system/media/camera/include -I$T/hardware/interfaces/common/support/include
 -I$T/frameworks/native/libs/binder/ndk/include_cpp -I$T/frameworks/native/libs/binder/ndk/include_ndk
 -I$T/frameworks/native/libs/binder/ndk/include_platform
 -I$T/system/libbase/include -I$T/system/core/libutils/include -I$T/system/core/libutils/binder/include
 -I$T/system/core/libcutils/include -I$T/system/logging/liblog/include -I$T/system/core/libsystem/include
 -I$T/system/core/libsync/include -I$T/system/libfmq/include -I$T/system/libfmq/base
 -I$T/system/libhidl/base/include -I$T/system/libhidl/transport/include -I$T/system/libhidl/libhidlmemory/include
 -I$T/system/libhidl/transport/token/1.0/utils/include
 -I$I/system/libhidl/transport/base/1.0/android.hidl.base@1.0_genc++_headers/gen
 -I$I/system/libhidl/transport/manager/1.0/android.hidl.manager@1.0_genc++_headers/gen
 -I$I/system/libhidl/transport/memory/1.0/android.hidl.memory@1.0_genc++_headers/gen
 -I$I/system/libhidl/transport/allocator/1.0/android.hidl.allocator@1.0_genc++_headers/gen
 -I$I/system/libhidl/transport/safe_union/1.0/android.hidl.safe_union@1.0_genc++_headers/gen
 -I$I/hardware/interfaces/graphics/mapper/2.0/android.hardware.graphics.mapper@2.0_genc++_headers/gen
 -I$I/hardware/interfaces/graphics/mapper/3.0/android.hardware.graphics.mapper@3.0_genc++_headers/gen
 -I$I/hardware/interfaces/graphics/mapper/4.0/android.hardware.graphics.mapper@4.0_genc++_headers/gen
 -I$I/hardware/interfaces/graphics/common/1.0/android.hardware.graphics.common@1.0_genc++_headers/gen
 -I$I/hardware/interfaces/graphics/common/1.1/android.hardware.graphics.common@1.1_genc++_headers/gen
 -I$I/hardware/interfaces/graphics/common/1.2/android.hardware.graphics.common@1.2_genc++_headers/gen
 -I$T/frameworks/native/libs/ui/include -I$T/frameworks/native/libs/ui/include_types
 -I$T/frameworks/native/libs/nativebase/include -I$T/frameworks/native/libs/arect/include
 -I$T/frameworks/native/libs/nativewindow/include -I$T/frameworks/native/libs/math/include
 -I$T/frameworks/native/libs/gralloc/types/include -I$T/frameworks/native/headers/media_plugin
 -I$T/frameworks/native/include -I$T/hardware/libhardware/include
 -I$T/external/libyuv/include -I$T/external/libjpeg-turbo -I$T/external/libexif -I$T/external/tinyxml2
 -I$CLANG/android_libc++/platform/aarch64/include/c++/v1
 -I$CLANG/include/c++/v1
 -isystem $T/bionic/libc/include -isystem $T/bionic/libc/kernel/uapi/asm-arm64 -isystem $T/bionic/libc/kernel/uapi
 -isystem $T/bionic/libc/kernel/android/scsi -isystem $T/bionic/libc/kernel/android/uapi"
compile() {
	src=$1; o=$OBJ/$(echo "$src" | sed "s|$W/||; s|$SRC/||; s|/|_|g; s|\.cpp$|.o|")
	if [ ! -e "$o" ] || [ "$src" -nt "$o" ]; then
		echo "CXX $src"
		nice -n 10 $CXX -c $CFLAGS $INC -o "$o" "$src"
	fi
}
if [ $# -gt 0 ]; then for f in "$@"; do compile "$f"; done; exit 0; fi
for f in $SRC/*.cpp $(find $W/gen/src -name '*.cpp'); do echo "$f"; done | xargs -P 2 -I{} "$0" "$T" "$W" {}
V=$P/vendor/lib64; S=$P/system/lib64
OUT=$W/out/vendor/bin/hw/android.hardware.camera.provider-V1-deckard-service
mkdir -p "$(dirname "$OUT")"
VLIBS="android.hardware.common-V2-ndk android.hardware.common.fmq-V1-ndk android.hardware.graphics.common-V7-ndk
 android.hardware.graphics.allocator-V2-ndk android.hardware.graphics.mapper@2.0 android.hardware.graphics.mapper@3.0
 android.hardware.graphics.mapper@4.0 android.hidl.allocator@1.0 android.hidl.memory@1.0 libbase libcamera_metadata
 libcutils libexif libfmq libgralloctypes libhardware libhidlbase libhidlmemory libjpeg libtinyxml2 libutils libui libyuv libc++"
SLIBS="libbinder_ndk liblog libsync"
BIONIC="$I/bionic/libc/libc/android_vendor_arm64_armv8-a_shared/libc.so $I/bionic/libm/libm/android_vendor_arm64_armv8-a_shared/libm.so $I/bionic/libdl/libdl/android_vendor_arm64_armv8-a_shared/libdl.so"
echo "LD $OUT"
nice -n 10 $CXX $I/bionic/libc/crtbegin_dynamic/android_vendor_arm64_armv8-a/crtbegin_dynamic.o $OBJ/*.o \
	$I/hardware/interfaces/camera/common/default/android.hardware.camera.common@1.0-helper/android_vendor_arm64_armv8-a_static/android.hardware.camera.common@1.0-helper.a \
	$I/hardware/interfaces/common/support/libaidlcommonsupport/android_vendor_arm64_armv8-a_static/libaidlcommonsupport.a \
	$CLANG/android_libc++/platform/aarch64/lib/libc++demangle.a \
	$CLANG/lib/clang/22/lib/linux/libclang_rt.ubsan_minimal-aarch64-android.a \
	-Wl,--start-group $CLANG/lib/clang/22/lib/linux/libclang_rt.builtins-aarch64-android.a -Wl,--end-group \
	$(for l in $VLIBS; do echo "$V/$l.so"; done) $(for l in $SLIBS; do echo "$S/$l.so"; done) $BIONIC \
	$I/bionic/libc/crtend_android/android_vendor_arm64_armv8-a/crtend_android.o \
	-o "$OUT.unstripped" -target aarch64-linux-android10000 -Wl,-z,noexecstack -Wl,-z,relro -Wl,-z,now \
	-Wl,--build-id=md5 -Wl,--fatal-warnings -Wl,--no-undefined-version -Wl,--exclude-libs,libgcc.a \
	-Wl,--exclude-libs,libunwind_llvm.a -Wl,--exclude-libs,libunwind.a -fuse-ld=lld -Wl,--icf=safe \
	-Wl,--no-undefined -Wl,-z,separate-code -Wl,-z,separate-loadable-segments -Wl,-z,max-page-size=4096 \
	-Wl,--pack-dyn-relocs=android+relr -pie -nostdlib -Bdynamic -Wl,--gc-sections -Wl,-z,nocopyreloc \
	-Wl,--exclude-libs=libclang_rt.builtins-aarch64-android.a -Wl,--exclude-libs=libclang_rt.ubsan_minimal-aarch64-android.a \
	-Wl,-dynamic-linker,/system/bin/linker64
$CLANG/bin/llvm-strip --strip-all -o "$OUT" "$OUT.unstripped"
mkdir -p "$W/out/vendor/etc/init" "$W/out/vendor/etc/vintf/manifest"
cp "$SRC/android.hardware.camera.provider-V1-deckard-service.rc" "$W/out/vendor/etc/init/"
cp "$SRC/android.hardware.camera.provider-V1-deckard-service.xml" "$W/out/vendor/etc/vintf/manifest/"
