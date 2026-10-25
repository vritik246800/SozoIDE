<p align="center">
  <img src="assets/Sozo.png" width="120" alt="SozoIDE logo: a cherry blossom">
</p>

<h1 align="center">SozoIDE</h1>

<p align="center">
  An editor, two terminals, a Git browser and a SQL client in one terminal window.<br>
  Written in Rust, for Windows, macOS and Linux.
</p>

<p align="center">
  <a href="https://github.com/vritik246800/SozoIDE/actions/workflows/ci.yml"><img src="https://github.com/vritik246800/SozoIDE/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI status"></a>
  <a href="https://github.com/vritik246800/SozoIDE/releases/latest">Download</a> ·
  <a href="https://vritik246800.github.io/SozoIDE/">Website</a> ·
  <a href="LICENSE">Apache-2.0</a>
</p>

## Features

- **Explorer and editor** — open files in tabs; edits save themselves after two idle seconds and before you switch tabs or quit. Syntax highlighting for more than 30 languages, from Rust and Go to SQL, Astro and Mermaid. Markdown tables are shown aligned, and images render inline on Sixel, Kitty and iTerm2 terminals.
- **Two terminals** — a bottom and a right terminal, each with its own shell, with scrollback on the mouse wheel or Page Up and Page Down. Drag the dividers to resize every panel.
- **Source Control** — stage and commit, follow the commit graph and branches, read diffs and the history of a line or a file, and check repository health. Pull is fast-forward only; rebase and checkout ask before they run.
- **Database** — connect to SQLite, PostgreSQL and MySQL, run queries with `Ctrl+Enter` or `F5`, and read rows, affected counts, duration and history. Saved connections never write passwords to disk.
- **Search** — in the open file or across the whole folder; files listed in `.gitignore` stay out of the results.
- **Your terminal's look** — SozoIDE has no colors of its own. It draws with the 16 ANSI colors of your terminal theme.

## Install

Download the file for your system from the [latest release](https://github.com/vritik246800/SozoIDE/releases/latest). Every installer puts the `sozo` command on your PATH.

| System | File | Install |
| --- | --- | --- |
| macOS (Apple silicon) | `SozoIDE-<version>-arm64.pkg` | Open it, or run `sudo installer -pkg SozoIDE-*.pkg -target /`. Installs `SozoIDE.app` and `/usr/local/bin/sozo`. |
| Windows (x86_64) | `SozoIDE-<version>-x86_64.msi` | Open it. Installs for your user only, without administrator rights. |
| Debian, Ubuntu | `sozoide_<version>-1_amd64.deb` | `sudo apt install ./sozoide_*.deb` |
| Fedora, openSUSE | `sozoide-<version>-1.x86_64.rpm` | `sudo dnf install ./sozoide-*.rpm` |
| Other Linux (x86_64) | `SozoIDE-<version>-x86_64-linux.tar.gz` | Extract `sozo` into a folder on your PATH. |

The installers are not signed yet:

- **macOS:** if the package is blocked, open System Settings, Privacy & Security, and choose Open Anyway.
- **Windows:** if SmartScreen appears, choose More info, then Run anyway.

Each release includes `SHA256SUMS.txt`. Check a download with `shasum -a 256 -c SHA256SUMS.txt --ignore-missing` on macOS, `sha256sum -c SHA256SUMS.txt --ignore-missing` on Linux, or `Get-FileHash` on Windows.

**Requirements:** `git` on your PATH for fetch, pull and push, and a [Nerd Font](https://www.nerdfonts.com/) in your terminal for the icons.

## Usage

```sh
sozo                   # open the current folder
sozo path/to/project   # open another folder
sozo --help
sozo --version
```

| Key | Action |
| --- | --- |
| `Ctrl+B` | Show or hide the Explorer |
| `Ctrl+J` | Show or hide the bottom terminal |
| `Ctrl+Shift+B` | Show or hide the right terminal |
| `Ctrl+G` | Open or close Source Control |
| `Ctrl+D` | Open or close Database |
| `Ctrl+P` | Search the open file |
| `Ctrl+Shift+P` | Search the whole folder |
| `Ctrl+W` | Close the active tab |
| `Ctrl+Enter` or `F5` | Run the SQL query |
| `F1` | Help with every command |
| `F2` | About, with the version |
| `Ctrl+Q` | Quit and restore the terminal |
| `Tab` / `Shift+Tab` | Move between tabs or fields |
| `Enter` | Open, activate or confirm |
| `Esc` | Close the dialog or panel |
| `Page Up` / `Page Down` | Scroll the editor or the terminal history |
| Mouse wheel | Scroll panels and terminal history |
| Drag a divider | Resize the Explorer and the terminals |

## Build from source

You need Rust stable and Git.

```sh
git clone https://github.com/vritik246800/SozoIDE.git
cd SozoIDE
cargo build --release
./target/release/sozo .
```

Before sending changes, run:

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

### Native packages

| System | Command | Output |
| --- | --- | --- |
| macOS | `bash packaging/build-macos.sh` | Apple silicon `.pkg` in `target/package/macos` |
| Linux | `bash packaging/build-linux.sh` | `.deb`, `.rpm` and `.tar.gz` in `target/package/linux` |
| Windows | `powershell -ExecutionPolicy Bypass -File packaging/build-windows.ps1` | `.msi` in `target/wix` |

Packaging tools are installed into `target/tools`, never globally.

### Website

```sh
cd site
npm ci
npm run dev
```

## Project layout

```text
src/          the application: app, editor, terminal, git, database, ui, animation
tests/        CLI, end-to-end and interaction tests in a real PTY
packaging/    native installers for Linux, macOS and Windows
site/         landing page (Astro), published to GitHub Pages
assets/       logo
.github/      CI, release and Pages workflows, and repository rulesets
.githooks/    local pre-commit and pre-push hooks
```

## Contributing

- Development happens on the `preview` branch. `main` only changes through pull requests with green CI.
- Open pull requests against `preview`.
- After cloning, turn on the local hooks: `git config core.hooksPath .githooks`
- Commit messages start with `create:`, `update:`, `fix:` or `docs:`, followed by a short summary in English.

## Releases

1. Raise `version` in `Cargo.toml` on `preview` and merge it into `main` through a pull request.
2. Tag `main` with the same version and push the tag: `git tag v0.1.0 && git push origin v0.1.0`
3. The Release workflow builds every installer and publishes the release with install notes and `SHA256SUMS.txt`.

## Security

Report vulnerabilities privately on GitHub, under Security, Report a vulnerability. Please do not open a public issue for them.

## License

[Apache License 2.0](LICENSE) © Vritik Valabdas
