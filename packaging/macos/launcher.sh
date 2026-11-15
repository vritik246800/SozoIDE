#!/bin/bash
# Opening SozoIDE.app installs the `sozo` command once (with a macOS dialog) and opens Terminal.
# The paths of the app never go inside the text of a command: they are passed as arguments and
# quoted by AppleScript (`quoted form of`), so a folder with a quote or a space cannot change
# what runs, least of all with administrator privileges.
set -euo pipefail
CONTENTS="$(cd "$(dirname "$0")/.." && pwd)"
APP="$(dirname "$CONTENTS")"
SOZO="$CONTENTS/MacOS/sozo"
BIN=/usr/local/bin
LINK="$BIN/sozo"
OSASCRIPT=/usr/bin/osascript

# The second button cancels: osascript then exits with an error.
dialog() {
    "$OSASCRIPT" -e "display dialog \"$1\" with title \"SozoIDE\" buttons {$2} default button 1 cancel button $3" \
        >/dev/null 2>&1
}

if [[ "$APP" == /Volumes/* ]]; then
    dialog "Drag SozoIDE to the Applications folder first, then open it from there." '"OK"' 1 || true
    exit 0
fi

if [[ "$(readlink "$LINK" 2>/dev/null)" != "$SOZO" ]]; then
    if dialog "Install the sozo command? Then run sozo . in any terminal folder to open it in SozoIDE." \
        '"Install", "Not Now"' 2; then
        if [[ -w "$BIN" ]]; then
            xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true
            ln -sf "$SOZO" "$LINK"
        else
            "$OSASCRIPT" \
                -e 'on run argv' \
                -e 'set theApp to quoted form of item 1 of argv' \
                -e 'set theBinary to quoted form of item 2 of argv' \
                -e 'set theFolder to quoted form of item 3 of argv' \
                -e 'set theLink to quoted form of item 4 of argv' \
                -e 'do shell script "xattr -dr com.apple.quarantine " & theApp & "; mkdir -p " & theFolder & " && ln -sf " & theBinary & " " & theLink with administrator privileges' \
                -e 'end run' \
                -- "$APP" "$SOZO" "$BIN" "$LINK" >/dev/null 2>&1 || true
        fi
    fi
fi

if [[ "$(readlink "$LINK" 2>/dev/null)" == "$SOZO" ]]; then
    COMMAND=sozo
else
    COMMAND="$SOZO"
fi
"$OSASCRIPT" \
    -e 'on run argv' \
    -e 'tell application "Terminal"' \
    -e 'activate' \
    -e 'do script (quoted form of item 1 of argv) & " ."' \
    -e 'end tell' \
    -e 'end run' \
    -- "$COMMAND"
