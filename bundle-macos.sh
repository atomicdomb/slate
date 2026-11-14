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

cargo build --release

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/slate "$APP/Contents/MacOS/slate"

# AppIcon.png -> AppIcon.icns, with the sizes Finder and the Dock ask for.
ICONSET=$(mktemp -d)/AppIcon.iconset
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z $size $size assets/AppIcon.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    sips -z $((size * 2)) $((size * 2)) assets/AppIcon.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

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
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>    <string>Text or source file</string>
            <key>CFBundleTypeRole</key>    <string>Editor</string>
            <key>LSHandlerRank</key>       <string>Alternate</string>
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
    echo "Installed /Applications/Slate.app"
fi
