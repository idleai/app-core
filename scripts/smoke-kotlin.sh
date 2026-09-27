#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

case "$(uname -s)" in
    Linux)
        jni_platform=linux
        library=libapp_core_bindings_jni.so
        linker=(-shared -Wl,-z,defs '-Wl,-rpath,$ORIGIN')
        ;;
    Darwin)
        jni_platform=darwin
        library=libapp_core_bindings_jni.dylib
        linker=(-dynamiclib '-Wl,-rpath,@loader_path')
        ;;
    *) echo "Kotlin/JNI smoke supports Linux and macOS hosts" >&2; exit 1 ;;
esac

# JAVA_HOME must name a full JDK: a JRE alone has no JNI headers.
: "${JAVA_HOME:?Set JAVA_HOME to a JDK 21 or newer}"
test -f "$JAVA_HOME/include/jni.h"
test -f "$JAVA_HOME/include/$jni_platform/jni_md.h"

# Build BoltFFI's generated JNI glue against the generated ABI header and the
# local Rust library. Both libraries live together so the loader needs no SDK.
"${CC:-cc}" -fPIC "${linker[@]}" \
    -I"$JAVA_HOME/include" -I"$JAVA_HOME/include/$jni_platform" \
    dist/native/kotlin/jni/jni_glue.c \
    -Ldist/native -lapp_core_bindings -o "dist/native/$library"

kotlinc dist/native/kotlin/ai/idle/appcore/bindings/AppCoreBindings.kt \
    dist/types/kotlin/ai dist/types/kotlin/com scripts/kotlin-smoke/Main.kt \
    -jvm-target 21 -include-runtime -d dist/native/kotlin-smoke.jar
"$JAVA_HOME/bin/java" -Xcheck:jni -Djava.library.path="$PWD/dist/native" \
    -jar dist/native/kotlin-smoke.jar
