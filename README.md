<p align="center">
  <img src="assets/Sozo.png" width="120" alt="SozoIDE logo: a cherry blossom">
</p>

<h1 align="center">SozoIDE</h1>

<p align="center">
  An IDE in your terminal: editor with IntelliSense, two terminals, Git, a SQL client,<br>
  run with one key, pictures, videos and 3D models. One Rust binary for Windows, macOS and Linux.
</p>

<p align="center">
  <a href="https://github.com/vritik246800/SozoIDE/actions/workflows/ci.yml"><img src="https://github.com/vritik246800/SozoIDE/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI status"></a>
  <a href="#install">Download</a> ·
  <a href="https://vritik246800.github.io/SozoIDE/">Website</a> ·
  <a href="LICENSE">Apache-2.0</a>
</p>

## Why SozoIDE

- **One binary, three systems.** `sozo` runs the same on Windows, macOS and Linux, on x86_64 and ARM64, and installs with a native installer.
- **Fast while you type.** Highlighting is redone only from the line you changed (at most a few tens of milliseconds a key on long files), a frame draws in under a millisecond, and language servers, Git, builds and videos run in their own processes or threads: the editor never waits for them.
- **Your terminal's look.** SozoIDE has no colours of its own: it draws with the 16 ANSI colours of your terminal theme and shows every shortcut the way your system types it.
- **Nothing to set up.** The tools it needs (`git`, `curl`, `ffmpeg` and the language servers) are installed in the background the first time it opens.

## Features

### Explorer

- Every file and folder with an icon for its type, in your terminal's colours.
- Right-click to create a file (its icon follows the extension you type) or a folder, to rename, to delete (it asks first), and to cut, copy and paste files and folders; `Ctrl+X`, `Ctrl+C` and `Ctrl+V` do the same on the selected entry. A copy in the same folder becomes `name copy`, `name copy 2`…; a move never overwrites anything.
- The tree follows other programs: what they create, rename or delete shows up within a second.
- Each folder opens as you left it, like VS Code: the folders you had open, your tabs with their cursors, the active tab and the panels.

### Editor

- Tabs with a scrolling tab bar; drag a tab to an edge of the editor to split it side by side or stacked, each side with its own tabs.
- Edits save themselves after two idle seconds and before you switch tabs or quit. A file changed by another program is read again (your unsaved edits are kept).
- Select with the mouse or `Shift` and the arrows, `Ctrl+A` for everything; copy, cut and paste with `Ctrl+C`, `Ctrl+X`, `Ctrl+V` or the right-click menu, through the system clipboard (and OSC 52 over SSH). `Ctrl+Z` undoes, a word or a paste at a time.
- Syntax highlighting for more than 30 languages, from Rust, Go, C, Java and Python to SQL, JSP, Astro and Mermaid, and for configuration files by their name (`.env`, `Cargo.lock`, `.config`, `.properties`, `Dockerfile`, `Makefile`, `.gitignore`…).
- Markdown drawn as you read it: headings, quotes, rules, framed code blocks, aligned tables with inline formatting, *italic*, **bold** and ==highlight==, and Mermaid diagrams in box-drawing characters (flowchart, sequence, class, state, C4, ZenUML, Kanban, Radar, Treemap and more). The source comes back under the cursor or the mouse.
- `Ctrl+P` finds in the file as you type and marks every match (`◂ 3/12 ▸`); `Ctrl+Shift+P` searches the whole folder in the background, groups the results by file and skips what `.gitignore` lists.

### IntelliSense

Problems underlined in red (errors) or yellow (warnings), with the message of the cursor's line in the footer; suggestions with `Ctrl+Space`, and by themselves after `.`, `::`, `->` and the second letter of a word; `F12` goes to the definition. They come from the language server of each language, started when you open its first file:

| Language | Server |
| --- | --- |
| Rust | rust-analyzer |
| C, C++ | clangd |
| Python | pyright |
| Java | jdtls |
| TypeScript, JavaScript | typescript-language-server |
| Go | gopls |

### Run

`Ctrl+R` saves everything, opens the bottom terminal and runs the file there with the tool of its language; a Cargo project or a Go module runs whole. The command stays in the shell's history, so you can run it again.

| Language | Command |
| --- | --- |
| Rust | `cargo run` (in a Cargo project) or `rustc` |
| C, C++ | `gcc`/`clang`, `g++`/`clang++` |
| Python | `python3` (`python` on Windows) |
| JavaScript, TypeScript | `node`; `deno`, `bun` or `npx tsx` |
| Java, Kotlin | `java`, `kotlinc` |
| Go | `go run` |
| Ruby, PHP, Lua, Perl, R, Swift, Dart, Zig, Haskell, Elixir, Julia | their own interpreter or compiler |
| Shell, PowerShell, Batch | `bash`, `zsh`, `pwsh`, `cmd.exe` |

### Debug

`F9` puts a breakpoint (`●`) on the cursor's line. `F5` saves, builds the file with debug symbols in the background (a Cargo project with `cargo build`, a single Rust, C or C++ file with `rustc -g`, `clang -g` or `gcc -g`) and starts the debugger; when it stops, the editor jumps to the line (`▶`) and a panel on the right shows the call stack, the variables and the program's output. `F5` continues, `F10` steps over, `F11` steps into and `Shift+F5` stops.

| Language | Debugger (Debug Adapter Protocol) |
| --- | --- |
| Rust, C, C++ | lldb-dap (comes with Xcode and LLVM), or GDB 14 and later (`gdb -i dap`) |
| Python | debugpy |
| Go | Delve (`dlv dap`) |

### Pictures, videos, 3D models and PDF

- Pictures (PNG, JPEG, GIF, WebP, BMP, ICO, TIFF, TGA, PNM, QOI, HDR), animated GIFs and videos (MP4, MOV, MKV, WebM, AVI, M4V) are drawn in half blocks in their own colours, two pixels a cell, on any terminal. Videos play through the system's `ffmpeg` at 15 frames a second, in your terminal's picture protocol when it has one (Kitty, Sixel, iTerm2), with sound through `ffplay`: a bar under the video shows the time and has buttons to play, pause, stop, seek, change the volume and the speed and fill the screen; click or drag the bar to seek, click the video to pause, and the wheel changes the volume.
- 3D models (`.glb`, `.gltf`) open in a tab, lit and with their colours: `←`/`→` turn them, `↑`/`↓` tilt them, `+`/`-` or the wheel zoom, and a drag turns them with the mouse.
- PDF files open one page at a time. A page with only text shows its text, sharp, to select, copy and search with `Ctrl+P`; a page with a picture, a diagram or another drawing shows whole as a small picture through your terminal's picture protocol (Sixel, Kitty or iTerm2), or half blocks without one.

### Two terminals

A bottom and a right terminal, each with its own shell, with scrollback on the mouse wheel or Page Up and Page Down. Drag the dividers to resize every panel.

### Source Control

Stage and commit, and scroll back through a timeline of every branch, each in its own colour with dots and merge lines: click a commit to slide in its files, and a file to see what was added (green), removed (red) or modified (yellow). Every branch in a tree grouped by remote and folder; click one to switch to it. The Launchpad shows your branch, its upstream, how far ahead and behind it is, and the staged, unstaged and untracked files. The diff numbers its lines, folds its hunks, goes side by side and moves to the next changed file. Pull is fast-forward only; rebase and checkout ask before they run.

### Database

Connect to SQLite, PostgreSQL and MySQL and the tables and views appear on the left. Draw some as a diagram with their keys and relations, tick columns to build the query (group, count, sum, average, sort and filter) and get the `SELECT` with its joins in the SQL tab, ready to edit, copy or run with `Ctrl+Enter` or `F5`. The rows scroll both ways. Passwords stay in your system's keychain, never on disk.

### A guided start

A short tutorial on the first start (skip it with `Esc`), and after an update a note with what is new in every version since the one you had. `F1` lists every key in boxed sections; the footer shows what just happened.

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

### Requirements

- A [Nerd Font](https://www.nerdfonts.com/) in your terminal for the icons.
- SozoIDE runs `git` (Source Control), `curl` (updates), `ffmpeg` with `ffprobe` and `ffplay` (videos and their sound) and the language servers instead of linking them. After a new installation and after each update it installs the ones that are missing, in the background:

| Tool | macOS | Windows | Linux |
| --- | --- | --- | --- |
| `git`, `curl`, `ffmpeg` | Homebrew | `winget` | `apt-get` or `dnf` (also recommended by the DEB and RPM packages) |
| rust-analyzer | `rustup` or Homebrew | `rustup` | `rustup` |
| clangd | Xcode command line tools | `winget` (LLVM) | `apt-get` or `dnf` |
| pyright, typescript-language-server | `npm` (or `pip` for pyright) | `npm` | `npm` |
| gopls | `go install` | `go install` | `go install` |
| jdtls | Homebrew | — | Homebrew |
| Debuggers | lldb-dap with Xcode; debugpy with `pip`; Delve with `go install` or Homebrew | lldb-dap with LLVM; debugpy with `pip`; Delve with `go install` | GDB with `apt-get` or `dnf`; debugpy with `pip`; Delve with `go install` |

On Linux the background install only works when `sudo` needs no password; otherwise the footer tells you what to install. Nothing is installed in debug builds or tests.

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
| Right-click in the Explorer | New file, new folder, cut, copy, paste, rename, delete (after a confirmation) |
| `Ctrl+X` / `Ctrl+C` / `Ctrl+V` in the Explorer | Cut / copy / paste the selected file or folder (paste goes into the selected folder) |
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
| `Ctrl+Z` (also on macOS) | Undo the last edit in the editor |
| `Ctrl+C` / `Ctrl+X` | Copy / cut the selection to the system clipboard |
| `Ctrl+V`, right-click in the editor | Paste the system clipboard; the menu also cuts and copies the selection |
| `←`/`→`, `↑`/`↓`, `+`/`-`, drag, wheel (3D model) | Turn, tilt, zoom in and out |
| `Space`, `S` (video) | Play or pause (at the end, play again); stop at the start |
| `←` / `→`, `Shift` + `←` / `→` (video) | Back / forward 5 seconds, 30 seconds |
| `-` / `+` or wheel, `M`, `[` / `]` (video) | Volume down / up, mute, slower / faster |
| `F`, `Esc` (video) | Full screen; leave it, then close the video |
| `Ctrl+Space` / `F12` | Suggestions from the language server (they also open after `.`, `::`, `->` and the second letter of a word) / go to the definition |
| `F9` / `F5` / `F10` / `F11` / `Shift+F5` | Breakpoint / build and debug, or continue / step over / step into / stop |
| `Ctrl+R` (editor or Explorer) | Save, open the bottom terminal and run the file there: a Cargo project or Go module runs whole; C, C++, Rust, Python, JavaScript, TypeScript, Java, Kotlin, Go, Ruby, PHP, Lua, Perl, R, Swift, Dart, Zig, Haskell, Elixir, Julia and shell scripts with their own compiler or interpreter |
| `Page Down` / `→` / mouse wheel, `Page Up` / `←`, `Home` / `End` (PDF) | Next page, previous page, first and last page; on a page of text the arrows move the cursor, `Shift` selects, `Ctrl+C` copies and the wheel scrolls on to the next page |
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
cargo clippy --all-targets -- -D warnings
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
src/          the application:
  app/        state, keys, mouse, Explorer, IntelliSense list, each folder's session
  editor/     text, highlighting, Markdown, Mermaid, PDF, search
  lsp/        language server client (JSON-RPC over stdio)
  run/        the command Ctrl+R runs
  debug/      the debugger client (Debug Adapter Protocol)
  media/      pictures, GIFs, videos and 3D models in half blocks
  terminal/   the two PTY terminals
  git/        Source Control
  database/   SQLite, PostgreSQL and MySQL
  update/     updates and the tools installed in the background
  ui/         drawing
  animation/  transitions
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
