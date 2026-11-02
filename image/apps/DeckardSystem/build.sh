#!/bin/sh
# Runs SettingsResultTest on the host JVM, then builds DeckardSystem.apk,
# signed with AOSP's platform test key (the image's test-keys build trusts it
# as android.uid.system). The key is AOSP's public
# build/target/product/security/platform, kept outside git.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
sdk=${ANDROID_HOME:-$HOME/android-sdk}
jar=$sdk/platforms/android-33/android.jar
export JAVA_HOME=${JAVA_HOME:-$(ls -d "$sdk"/jdk-17*)}
export PATH=$JAVA_HOME/bin:$PATH
test=$(mktemp -d)
trap 'rm -rf "$test"' EXIT
javac -source 1.8 -target 1.8 -Xlint:-options -bootclasspath "$jar" -d "$test" \
    "$here/src/org/marathonos/deckard/system/SettingsResult.java" $(find "$here/test" -name '*.java')
java -cp "$test" org.marathonos.deckard.system.SettingsResultTest
export PLATFORM_KEY=${PLATFORM_KEY:-$HOME/.marathon-secrets/deckard/aosp-test-keys/platform}
"$here/../../tools/build-apk.sh" "$here" "$@"
