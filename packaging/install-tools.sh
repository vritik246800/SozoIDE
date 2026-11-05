#!/usr/bin/env bash
# Installs the tools that make SozoIDE better, for new users, after an update or on a server
# without the Setup dialog. Safe to run again: what is there stays.
#
#   install-tools.sh [--check] [--yes] [tool ...]
#
#   --check   only says what is missing; exits with 1 when something is
#   --yes     does not ask before installing (for scripts)
#   tool      git, curl, ffmpeg, tmux (all of them by default)
#
# It only uses the package manager of this system (Homebrew, apt-get or dnf), and asks first.
# sudo runs only after that answer, and asks for its password in this terminal.
set -euo pipefail

check=false
yes=false
tools=()
for argument in "$@"; do
    case $argument in
    --check) check=true ;;
    --yes) yes=true ;;
    git | curl | ffmpeg | tmux) tools+=("$argument") ;;
    *)
        echo "Usage: install-tools.sh [--check] [--yes] [git|curl|ffmpeg|tmux ...]" >&2
        exit 2
        ;;
    esac
done
[[ ${#tools[@]} -gt 0 ]] || tools=(git curl ffmpeg tmux)

purpose() {
    case $1 in
    git) echo "Source Control" ;;
    curl) echo "updates of SozoIDE" ;;
    ffmpeg) echo "videos in the editor" ;;
    tmux) echo "shells that keep running after SozoIDE closes" ;;
    esac
}

missing=()
for tool in "${tools[@]}"; do
    if command -v "$tool" >/dev/null 2>&1; then
        echo "OK       $tool"
    else
        echo "Missing  $tool ($(purpose "$tool"))"
        missing+=("$tool")
    fi
done
[[ ${#missing[@]} -gt 0 ]] || exit 0
$check && exit 1

# The command that installs the missing tools here. Fedora calls FFmpeg ffmpeg-free.
sudo_word=()
[[ $(id -u) -eq 0 ]] || sudo_word=(sudo)
case $(uname -s) in
Darwin)
    command -v brew >/dev/null 2>&1 || {
        echo "Install Homebrew first: https://brew.sh" >&2
        exit 1
    }
    install=(brew install "${missing[@]}")
    ;;
Linux)
    if command -v apt-get >/dev/null 2>&1; then
        install=(${sudo_word[@]+"${sudo_word[@]}"} apt-get install -y "${missing[@]}")
    elif command -v dnf >/dev/null 2>&1; then
        install=(${sudo_word[@]+"${sudo_word[@]}"} dnf install -y "${missing[@]/#ffmpeg/ffmpeg-free}")
    else
        echo "No apt-get or dnf here: install ${missing[*]} with your package manager" >&2
        exit 1
    fi
    ;;
*)
    echo "Unknown system: install ${missing[*]} by hand" >&2
    exit 1
    ;;
esac

echo "Will run: ${install[*]}"
if ! $yes; then
    read -r -p "Install? [y/N] " answer || answer=
    [[ $answer == [yY]* ]] || {
        echo "Nothing installed. SozoIDE works without them."
        exit 0
    }
fi
"${install[@]}"

failed=0
for tool in "${missing[@]}"; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "$tool is still missing after the installation" >&2
        failed=1
    fi
done
exit $failed
