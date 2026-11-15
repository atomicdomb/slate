#!/bin/bash
# Build Slate.app for this Mac.
#
#   ./bundle-macos.sh              build target/Slate.app
#   ./bundle-macos.sh --install    ...and copy it to /Applications
#
# The app is signed "ad hoc" (no certificate), which is all macOS needs for
# something built and run on the same machine. To sign with your own
# certificate instead, name it:
#
#   SIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)" ./bundle-macos.sh
set -euo pipefail
cd "$(dirname "$0")"

APP=target/Slate.app
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
IDENTITY=${SIGN_IDENTITY:--}

# Cargo decides what to recompile from file dates alone, and files copied
# out of an archive can carry dates older than the last build. Mark Slate's
# own sources as changed so the app always matches them. (Only Slate itself
# is recompiled; the libraries it uses stay cached.)
find src assets build.rs Cargo.toml -type f -exec touch {} +

cargo build --release

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/slate "$APP/Contents/MacOS/slate"

# Turn a 1024 px PNG into an .icns with the sizes Finder and the Dock ask for.
make_icns() {
    local png=$1 name=$2
    local iconset
    iconset=$(mktemp -d)/$name.iconset
    mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
        sips -z $size $size "$png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
        sips -z $((size * 2)) $((size * 2)) "$png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$iconset" -o "$APP/Contents/Resources/$name.icns"
}
make_icns assets/AppIcon.png AppIcon            # the app itself
make_icns assets/DocumentIcon.png Document      # files that open in Slate

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>                <string>Slate</string>
    <key>CFBundleDisplayName</key>         <string>Slate</string>
    <key>CFBundleIdentifier</key>          <string>local.slate.editor</string>
    <key>CFBundleExecutable</key>          <string>slate</string>
    <key>CFBundleIconFile</key>            <string>AppIcon</string>
    <key>CFBundlePackageType</key>         <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key> <string>6.0</string>
    <key>CFBundleShortVersionString</key>  <string>$VERSION</string>
    <key>CFBundleVersion</key>             <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>      <string>11.0</string>
    <key>LSApplicationCategoryType</key>   <string>public.app-category.developer-tools</string>
    <key>NSHighResolutionCapable</key>     <true/>
    <key>NSPrincipalClass</key>            <string>NSApplication</string>
    <!-- What Finder may hand to Slate. "Alternate" means: offer Slate under
         Open With, but do not make it the default for anything. -->
    <!-- macOS has no type of its own for .rs files (it reports a made-up
         "dyn.…" one), so nothing ties their icon to an app. Slate defines
         the type here, with its document icon, and claims it below. -->
    <key>UTExportedTypeDeclarations</key>
    <array>
        <dict>
            <key>UTTypeIdentifier</key>    <string>local.slate.editor.rust-source</string>
            <key>UTTypeDescription</key>   <string>Rust source</string>
            <key>UTTypeIconFile</key>      <string>Document</string>
            <!-- Deliberately plain data, not "public.source-code": for text
                 types Finder shows a preview of the contents instead of the
                 icon. Change it back to get Quick Look text previews. -->
            <key>UTTypeConformsTo</key>
            <array>
                <string>public.data</string>
            </array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key>
                <array>
                    <string>rs</string>
                </array>
            </dict>
        </dict>
    </array>
    <key>CFBundleDocumentTypes</key>
    <array>
        <!-- The one type Slate makes itself the default app for. -->
        <dict>
            <key>CFBundleTypeName</key>    <string>Rust source</string>
            <key>CFBundleTypeRole</key>    <string>Editor</string>
            <key>LSHandlerRank</key>       <string>Owner</string>
            <key>CFBundleTypeIconFile</key> <string>Document</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>local.slate.editor.rust-source</string>
            </array>
        </dict>
        <!-- Source files by extension, because most (.rs among them) have
             no system-wide type of their own. The icon file is what Finder
             shows for a file once Slate is its default app. -->
        <dict>
            <key>CFBundleTypeName</key>    <string>Source code</string>
            <key>CFBundleTypeRole</key>    <string>Editor</string>
            <key>LSHandlerRank</key>       <string>Alternate</string>
            <key>CFBundleTypeIconFile</key> <string>Document</string>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>rs</string>
                <string>toml</string>
                <string>md</string>
                <string>markdown</string>
                <string>txt</string>
                <string>json</string>
                <string>jsonc</string>
                <string>ron</string>
                <string>yaml</string>
                <string>yml</string>
                <string>lock</string>
                <string>sh</string>
                <string>bash</string>
                <string>zsh</string>
                <string>py</string>
                <string>js</string>
                <string>ts</string>
                <string>jsx</string>
                <string>tsx</string>
                <string>c</string>
                <string>h</string>
                <string>cpp</string>
                <string>cc</string>
                <string>hpp</string>
                <string>cs</string>
                <string>go</string>
                <string>lua</string>
                <string>glsl</string>
                <string>wgsl</string>
                <string>hlsl</string>
                <string>vert</string>
                <string>frag</string>
                <string>shader</string>
                <string>html</string>
                <string>css</string>
                <string>xml</string>
                <string>ini</string>
                <string>cfg</string>
                <string>conf</string>
                <string>log</string>
                <string>csv</string>
            </array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key>    <string>Text or source file</string>
            <key>CFBundleTypeRole</key>    <string>Editor</string>
            <key>LSHandlerRank</key>       <string>Alternate</string>
            <key>CFBundleTypeIconFile</key> <string>Document</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.text</string>
                <string>public.data</string>
            </array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key>    <string>Folder</string>
            <key>CFBundleTypeRole</key>    <string>Editor</string>
            <key>LSHandlerRank</key>       <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.folder</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
PLIST

codesign --force --sign "$IDENTITY" "$APP"
echo "Built $APP"

if [[ "${1:-}" == "--install" ]]; then
    rm -rf /Applications/Slate.app
    cp -R "$APP" /Applications/Slate.app
    # Tell Finder about the file types the new copy accepts.
    LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
    [[ -x "$LSREGISTER" ]] && "$LSREGISTER" -f /Applications/Slate.app || true
    echo "If file icons in Finder look out of date, log out and back in: Finder keeps them cached."
    echo "Installed /Applications/Slate.app"
fi
