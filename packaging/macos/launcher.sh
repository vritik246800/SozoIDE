#!/bin/bash
# Opening SozoIDE.app installs the `sozo` command once (with a macOS dialog) and opens Terminal.
set -euo pipefail
CONTENTS="$(cd "$(dirname "$0")/.." && pwd)"
APP="$(dirname "$CONTENTS")"
SOZO="$CONTENTS/MacOS/sozo"
LINK=/usr/local/bin/sozo

# The second button cancels: osascript then exits with an error.
dialog() {
    /usr/bin/osascript -e "display dialog \"$1\" with title \"SozoIDE\" buttons {$2} default button 1 cancel button $3" \
        >/dev/null 2>&1
}

if [[ "$APP" == /Volumes/* ]]; then
    dialog "Drag SozoIDE to the Applications folder first, then open it from there." '"OK"' 1 || true
    exit 0
fi

if [[ "$(readlink "$LINK" 2>/dev/null)" != "$SOZO" ]]; then
    if dialog "Install the sozo command? Then run sozo . in any terminal folder to open it in SozoIDE." \
        '"Install", "Not Now"' 2; then
        INSTALL="xattr -dr com.apple.quarantine '$APP'; mkdir -p /usr/local/bin && ln -sf '$SOZO' '$LINK'"
        if [[ -w /usr/local/bin ]]; then
            /bin/bash -c "$INSTALL"
        else
            /usr/bin/osascript -e "do shell script \"$INSTALL\" with administrator privileges" \
                >/dev/null 2>&1 || true
        fi
    fi
fi

if [[ "$(readlink "$LINK" 2>/dev/null)" == "$SOZO" ]]; then
    COMMAND=sozo
else
    COMMAND="'$SOZO'"
fi
/usr/bin/osascript -e 'tell application "Terminal" to activate' \
    -e "tell application \"Terminal\" to do script \"$COMMAND .\""
