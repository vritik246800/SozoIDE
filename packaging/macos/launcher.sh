#!/bin/bash
set -euo pipefail
if [[ -x /usr/local/bin/sozo ]]; then
    SOZO=/usr/local/bin/sozo
else
    SOZO="$(dirname "$0")/sozo"
fi
/usr/bin/osascript -e 'tell application "Terminal" to activate' \
    -e "tell application \"Terminal\" to do script \"$SOZO .\""
