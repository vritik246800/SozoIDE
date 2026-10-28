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
  <a href="#install">Download</a> ·
  <a href="https://vritik246800.github.io/SozoIDE/">Website</a> ·
  <a href="LICENSE">Apache-2.0</a>
</p>

## Features

- **Explorer and editor** — every file shows an icon for its type; right-click to create a file (its icon follows the extension you type) or a folder, or to rename one; open files in tabs (the tab bar scrolls when there are many) and drag a tab to an edge of the editor to split it side by side or stacked, each side with its own tabs; drag tabs back to the other side to join them again. Long lines scroll sideways. Select text with the mouse or `Shift` and the arrows, `Ctrl+A` for everything, and copy or cut it with `Ctrl+C` and `Ctrl+X`. Edits save themselves after two idle seconds and before you switch tabs or quit. Syntax highlighting for more than 30 languages, from Rust and Go to SQL, JSP, Astro and Mermaid, and for configuration files by their name (`.env`, `Cargo.lock`, `.config`, `.properties`, `Dockerfile`, `Makefile`, `.gitignore`…), with its own colour for keywords, types, functions, strings, numbers and comments, down to the last line of long files. Markdown is drawn as you read it: headings in their own size and colour, quotes, rules, framed code blocks, Mermaid diagrams in box-drawing characters (flowcharts, sequence, class, state, C4, ZenUML, Kanban, Radar, Treemap and more), aligned tables with inline formatting, typographic quotes, *italic*, **bold** and ==highlight== — and the source comes back under the cursor or the mouse. Images render inline on Sixel, Kitty and iTerm2 terminals, and PDF files open in a tab as they are, one page at a time. `Ctrl+P` finds in the file as you type, marks every match in the editor and counts them (`◂ 3/12 ▸`); `Ctrl+Shift+P` searches the whole folder in the background and groups the results by file with the match highlighted.
- **Two terminals** — a bottom and a right terminal, each with its own shell, with scrollback on the mouse wheel or Page Up and Page Down. Drag the dividers to resize every panel.
- **Source Control** — stage and commit, scroll back through a timeline of every branch, each in its own colour with dots and merge lines — click a commit to slide in its files, and a file to see what was added (green), removed (red) or modified (yellow) — and see every branch in a tree grouped by remote and folder: click one to switch to it. The Launchpad shows your branch, its upstream, how far ahead and behind it is, and the staged, unstaged and untracked files, each a click away from what resolves it. The diff numbers its lines, folds its hunks, goes side by side, and moves to the next or previous changed file. Pull is fast-forward only; rebase and checkout ask before they run.
- **Database** — connect to SQLite, PostgreSQL and MySQL and the tables and views appear on the left. Pick some to draw them as a diagram with their keys and relations, tick columns to build the query — group, count, sum, average, sort and filter — and get the `SELECT` with its joins in the SQL tab, ready to edit, copy or run with `Ctrl+Enter` or `F5`. The rows come out below and scroll both ways. Saved connections never write passwords to disk.
- **Search** — in the open file or across the whole folder; files listed in `.gitignore` stay out of the results.
- **Your terminal's look** — SozoIDE has no colors of its own. It draws with the 16 ANSI colors of your terminal theme, and shows each shortcut the way your system types it (`fn+F1` and `Ctrl+Option+B` on macOS, `Ctrl+Alt+B` on Linux).
- **A guided start** — a short tutorial on the first start (skip it with `Esc`), and after an update a note with what is new and what was fixed in every version since the one you had. `F1` lists every key in boxed sections, in your terminal's colours.

## Install

Download the installer for your system and open it. Then open a new terminal in any folder and run `sozo .`

| System | Download | Install |
| --- | --- | --- |
| macOS (Apple silicon) | [SozoIDE-macos-arm64.dmg](https://github.com/vritik246800/SozoIDE/releases/latest/download/SozoIDE-macos-arm64.dmg) | Drag SozoIDE to Applications and open the app once: it offers to install the `sozo` command in `/usr/local/bin`. |
| Windows x86_64 | [SozoIDE-windows-x86_64.msi](https://github.com/vritik246800/SozoIDE/releases/latest/download/SozoIDE-windows-x86_64.msi) | Follow the setup wizard. Installs for your user only, without administrator rights, and adds `sozo` to your PATH. |
| Windows ARM64 | [SozoIDE-windows-arm64.msi](https://github.com/vritik246800/SozoIDE/releases/latest/download/SozoIDE-windows-arm64.msi) | Same as x86_64. |
| Debian, Ubuntu | [x86_64](https://github.com/vritik246800/SozoIDE/releases/latest/download/SozoIDE-linux-x86_64.deb), [ARM64](https://github.com/vritik246800/SozoIDE/releases/latest/download/SozoIDE-linux-arm64.deb) `.deb` | Open it in your software installer, or `sudo apt install ./SozoIDE-linux-*.deb` |
| Fedora, openSUSE | [x86_64](https://github.com/vritik246800/SozoIDE/releases/latest/download/SozoIDE-linux-x86_64.rpm), [ARM64](https://github.com/vritik246800/SozoIDE/releases/latest/download/SozoIDE-linux-arm64.rpm) `.rpm` | Open it in your software installer, or `sudo dnf install ./SozoIDE-linux-*.rpm` |
| Other Linux | [x86_64](https://github.com/vritik246800/SozoIDE/releases/latest/download/SozoIDE-linux-x86_64.tar.gz), [ARM64](https://github.com/vritik246800/SozoIDE/releases/latest/download/SozoIDE-linux-arm64.tar.gz) `.tar.gz` | Extract `sozo` into a folder on your PATH. |

The installers are not signed yet:

- **macOS:** if the app does not open, go to System Settings, Privacy & Security, and choose Open Anyway.
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
| `Ctrl+Shift+B` (macOS: `Ctrl+Option+B`, Linux: `Ctrl+Alt+B`) | Show or hide the right terminal |
| `Ctrl+G` | Open or close Source Control |
| `Ctrl+D` | Open or close Database |
| `Ctrl+P` | Search the open file |
| `Ctrl+Shift+P` | Search the whole folder |
| `Up` / `Down`, `◂` / `▸` (search) | Previous / next result; `Enter` opens it selected |
| Right-click in the Explorer | New file, new folder, rename |
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
| `Shift` + mouse wheel | Scroll the editor sideways |
| Mouse wheel on the tabs, `‹` / `›` | Scroll the tab bar |
| Drag a divider | Resize the Explorer and the terminals |
| Drag a tab | To an edge of the editor: split it, each side with its own tabs; to the other side: move it there (a side without tabs closes) |
| `Shift` + arrows, `Home`, `End`, or drag the mouse | Select text in the editor |
| `Ctrl+A` | Select all the text |
| `Ctrl+C` / `Ctrl+X` | Copy / cut the selection to the system clipboard |
| `Page Down` / `→` / mouse wheel, `Page Up` / `←`, `Home` / `End` (PDF) | Next page, previous page, first and last page |
| Click a branch (Source Control) | Switch to it, after a confirmation |
| Click a commit or `Enter` (Commit Graph) | Its files; `Enter` on a file: its diff; `[` / `]` other file; `Esc` back |
| `Enter` / `Space`, `F6`, `Delete`, `Shift` + wheel (Database) | Add a table, tick a column, change a builder cell; next part; remove the builder row; scroll the results sideways |
| `n` / `p`, `[` / `]`, `v`, `s` / `u`, `o` (Diff) | Next / previous hunk, previous / next changed file, side by side, stage / unstage, open the file |
| `Up` / `Down` or the mouse wheel (Help, What's new) | Scroll a dialog taller than the terminal |
| `G` (in About) | Open the repository to contribute |

### Motion

Panels grow from their divider, Source Control and Database draw in line by line, a tab comes in from its side, and dialogs open from their middle row with the rest of the screen dimmed. Transitions never hold up saving, commits or queries. In About the SozoIDE flower grows from its centre. Set `SOZO_REDUCED_MOTION=1` to turn them off.

### Updates

When a newer release is out, SozoIDE tells you at startup. Press `Enter` (or `U`) to update: SozoIDE downloads the installer for your system, installs it and restarts in the same folder. Press `S` to skip that version, or `Esc` to decide later. On Debian, Ubuntu and Fedora the update runs `sudo`, so it asks for your password. Set `SOZO_NO_UPDATE_CHECK=1` to turn the check off. The first start after an update shows what the new version brings.

### Privacy

On its first start SozoIDE counts the installation once: it downloads the small `SozoIDE-<system>-<arch>.installed.txt` file of its release, and GitHub's public download counter goes up by one. The first start after an update does the same with `SozoIDE-<system>-<arch>.updated.txt`. That counter only knows the system, the architecture and the version; nothing about you or your files is sent, and SozoIDE has no server of its own. Set `SOZO_NO_TELEMETRY=1` to turn both off. The startup check for updates also asks GitHub for the latest release (`SOZO_NO_UPDATE_CHECK=1` turns it off).

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
| macOS | `bash packaging/build-macos.sh` | Apple silicon `.dmg` in `target/package/macos` |
| Linux | `bash packaging/build-linux.sh` | `.deb`, `.rpm` and `.tar.gz` in `target/package/linux` |
| Windows | `powershell -ExecutionPolicy Bypass -File packaging/build-windows.ps1` | `.msi` in `target/wix` |

Packaging tools are installed into `target/tools`, never globally. `bash packaging/test-install.sh` (macOS, Linux) and `packaging/test-install.ps1` (Windows) install the package the way a user would and check `sozo --version`.

### Website

```sh
cd site
npm ci
npm run dev
```

## Project layout

```text
src/          the application: app, editor (highlight, Markdown, Mermaid, PDF), terminal, git, database, update, ui, animation
tests/        CLI, end-to-end and interaction tests in a real PTY
packaging/    native installers for Linux, macOS and Windows
site/         landing page (Astro), published to GitHub Pages; site/scripts captures its SozoIDE screens
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
3. The Release workflow builds every installer, installs it on its own system, and publishes the release with install notes and `SHA256SUMS.txt`. Package names carry no version, so `releases/latest/download/<name>` always points at the newest one.
4. Check that every download link on the website now delivers the new version: `bash packaging/check-downloads.sh`

## Security

Report vulnerabilities privately on GitHub, under Security, Report a vulnerability. Please do not open a public issue for them.

## License

[Apache License 2.0](LICENSE) © Vritik Valabdas
