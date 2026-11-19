#!/bin/sh
# Builds LiteHTMLParser.xcframework: the html5ever parser (core crate without unsafe)
# behind lh_html5_parse, prelinked so only lh_html5_* and a weak rust_eh_personality
# are exported, arm64 from 11.0 and x86_64 from 10.13. litehtml links it unless the
# manifest sees TWIDGET_HTML_PARSER=gumbo.
#
#   litehtml/html5/build-xcframework.sh [BUILD_DIR]

set -eu

ROOT="$(cd "$(dirname "$0")" && pwd)"
BUILD_DIR="${1:-${TMPDIR:-/tmp}/lh-html5-xcframework}"
mkdir -p "$BUILD_DIR"
BUILD_DIR="$(cd "$BUILD_DIR" && pwd)"
if [ -n "${CARGO:-}" ]; then
    CARGO_BIN="$CARGO"
elif command -v cargo >/dev/null 2>&1; then
    CARGO_BIN="$(command -v cargo)"
else
    CARGO_BIN="$HOME/.cargo/bin/cargo"
fi
RUSTUP_BIN="$(dirname "$CARGO_BIN")/rustup"

cd "$ROOT"
build() {
    target="$1"
    deployment="$2"
    arch="$3"
    if [ -x "$RUSTUP_BIN" ]; then
        "$RUSTUP_BIN" target add "$target" >/dev/null 2>&1 || true
    fi
    MACOSX_DEPLOYMENT_TARGET="$deployment" CARGO_TARGET_DIR="$BUILD_DIR/target" \
        "$CARGO_BIN" build --package lh-html5-ffi --release --locked --target "$target"
    printf '_lh_html5_*\n_rust_eh_personality\n' > "$BUILD_DIR/exported-symbols.txt"
    if ! ld -r -arch "$arch" -platform_version macos "$deployment" "$deployment" \
        -exported_symbols_list "$BUILD_DIR/exported-symbols.txt" \
        -all_load "$BUILD_DIR/target/$target/release/liblh_html5_ffi.a" \
        -o "$BUILD_DIR/lh_html5-$target.o" 2> "$BUILD_DIR/ld-$target.log"; then
        cat "$BUILD_DIR/ld-$target.log"
        exit 1
    fi
    strip -S "$BUILD_DIR/lh_html5-$target.o"
    if nm --no-llvm-bc -gU "$BUILD_DIR/lh_html5-$target.o" | grep -q ' _rust_eh_personality$'; then
        python3 -I "$ROOT/tools/weak_personality.py" "$BUILD_DIR/lh_html5-$target.o"
    fi
    rm -f "$BUILD_DIR/liblh_html5-$target.a"
    libtool -static -o "$BUILD_DIR/liblh_html5-$target.a" "$BUILD_DIR/lh_html5-$target.o"
    exported="$(nm --no-llvm-bc -gU "$BUILD_DIR/liblh_html5-$target.a" | awk 'NF == 3 { print $3 }' | grep -v -e '^_lh_html5_' -e '^_rust_eh_personality$' || true)"
    if [ -n "$exported" ]; then
        echo "symbols other than lh_html5_* are exported for $target:"
        echo "$exported"
        exit 1
    fi
}

build aarch64-apple-darwin 11.0 arm64
build x86_64-apple-darwin 10.13 x86_64

lipo -create "$BUILD_DIR/liblh_html5-aarch64-apple-darwin.a" "$BUILD_DIR/liblh_html5-x86_64-apple-darwin.a" \
    -output "$BUILD_DIR/liblh_html5.a"

HEADERS_DIR="$BUILD_DIR/headers"
rm -rf "$HEADERS_DIR"
mkdir -p "$HEADERS_DIR/LiteHTMLParser"
cp compat/lh_html5.h "$HEADERS_DIR/LiteHTMLParser/"
cat > "$HEADERS_DIR/LiteHTMLParser/module.modulemap" <<EOF
module LiteHTMLParser {
    header "lh_html5.h"
    export *
}
EOF

rm -rf "$ROOT/LiteHTMLParser.xcframework"
xcodebuild -create-xcframework -library "$BUILD_DIR/liblh_html5.a" -headers "$HEADERS_DIR" \
    -output "$ROOT/LiteHTMLParser.xcframework" >/dev/null
(cd "$ROOT" && find LiteHTMLParser.xcframework -type f | LC_ALL=C sort | xargs shasum -a 256 > LiteHTMLParser.sha256)
lipo -info "$BUILD_DIR/liblh_html5.a"
du -sh "$ROOT/LiteHTMLParser.xcframework"
