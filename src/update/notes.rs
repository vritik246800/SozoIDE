//! What each version brought, shown after an update (F18): the notes of every version newer
//! than the one that ran before, newest first. A new release adds its notes at the top; a test
//! keeps the first one equal to `Cargo.toml`.

pub struct Notes {
    pub version: &'static str,
    pub new: &'static [&'static str],
    pub fixed: &'static [&'static str],
}

pub const NOTES: &[Notes] = &[
    Notes {
        version: "0.7.1",
        new: &[
            "PDF: a page with only text shows its text, to select, copy and search with Ctrl+P; a page with a picture or a drawing shows whole as a small picture",
            "Videos play with sound (ffplay) and in your terminal's picture protocol when it has one; a bar under the video shows the time and has buttons",
            "Video keys: Space plays or pauses, S stops, arrows seek 5 s (30 s with Shift), - and + change the volume, M mutes, [ and ] change the speed, F fills the screen, Esc leaves it",
            "Video mouse: click the buttons, click or drag the bar to seek, click the video to pause, the wheel changes the volume",
            "Each folder opens as you left it: its open folders, tabs, cursors and panels",
            "About: Check for updates [U] looks for a new version now, also one you skipped",
        ],
        fixed: &[],
    },
    Notes {
        version: "0.7.0",
        new: &[
            "IntelliSense from language servers (rust-analyzer, clangd, pyright, jdtls, typescript-language-server, gopls): problems underlined, Ctrl+Space suggests, F12 goes to the definition",
            "Ctrl+R saves and runs the file, or its Cargo or Go project, in the bottom terminal",
            "Debug Rust, C, C++, Python and Go: F9 sets a breakpoint, F5 starts or continues, F10 and F11 step, Shift+F5 stops; the stack, the variables and the output show beside the editor",
            "3D models (.glb, .gltf) open in a tab: arrows turn and tilt, + and - or the wheel zoom, a drag turns them",
            "Pictures, animated GIFs and videos are drawn in half blocks in their own colours; videos play through ffmpeg and Space pauses",
            "Explorer: cut, copy and paste files and folders with a right-click or Ctrl+X, Ctrl+C and Ctrl+V",
            "Editor: right-click to cut, copy or paste; Ctrl+V pastes from the system clipboard",
            "Files and folders changed by other programs show up by themselves",
            "git, curl, ffmpeg, the language servers and the debuggers are installed in the background when they are missing",
        ],
        fixed: &[
            "Typing in long files, Markdown above all, no longer lags",
            "The footer shows what just happened, and the problem on the cursor's line",
        ],
    },
    Notes {
        version: "0.6.0",
        new: &[
            "Explorer: right-click a file or a folder and choose Delete; it asks first and says it cannot be undone",
            "Ctrl+Z undoes the last edit in the editor, on macOS too; a word typed or a paste undoes in one step",
        ],
        fixed: &[],
    },
    Notes {
        version: "0.5.0",
        new: &[
            "Explorer: right-click to create a file or a folder, or to rename one; the icon follows the extension you type",
            "Ctrl+P finds as you type in a bar under the editor: every match marked, the current one counted as 3/12 with arrows",
            "Ctrl+Shift+P searches the folder in the background and groups the results by file with the match highlighted",
            "Commit Graph is now a timeline of every branch in its own colour; click a commit to see its files and a file to see its diff",
            "In that diff, added lines are green, removed red and modified yellow",
            "Database: after connecting, tables and views on the left; draw them as a diagram with their keys and relations",
            "Tick columns to build a query: group, count, sum, average, min, max, sort and filter, with the joins made for you",
            "The SQL tab shows the query to edit, copy or run; the results scroll down and sideways",
        ],
        fixed: &[
            "PDF files and images show at once in Kitty, without switching tabs",
            "Queries with table functions such as pragma_table_info run in SQLite",
        ],
    },
    Notes {
        version: "0.4.0",
        new: &[
            "Source Control: Branches as a tree grouped by remote and folder; click a branch to switch to it",
            "Launchpad in cards: current branch, upstream, ahead and behind, staged, unstaged and untracked files",
            "Diff with line numbers, folding hunks, side by side view, next and previous file, stage, unstage and open",
            "Select text with the mouse or Shift and the arrows; Ctrl+A selects all, Ctrl+C and Ctrl+X copy",
            "PDF files open in a tab, one page at a time",
            "Mermaid: C4, ZenUML, Kanban, Radar and Treemap diagrams, class members after a colon and front matter",
            "Configuration files get their colours: .env, .lock, .config, .properties, Dockerfile, Makefile and more",
            "Help in boxed sections with the colours of your terminal; it scrolls when it does not fit",
            "About shows the flower in the middle, bigger when there is room",
            "These notes now list every version since the one you had",
        ],
        fixed: &[
            "Long files keep their colours to the end: a line comment no longer greys out the rest",
            "JSP fragments (.jspf) and .mjs, .cjs and .jsx files are highlighted",
        ],
    },
    Notes {
        version: "0.3.3",
        new: &[
            "Updates always install the latest release, even when a newer one comes out while SozoIDE is open",
        ],
        fixed: &[],
    },
    Notes {
        version: "0.3.2",
        new: &[
            "Updates are now counted once too, like new installations, anonymously (system, architecture and version only); SOZO_NO_TELEMETRY=1 turns both off",
        ],
        fixed: &[],
    },
    Notes {
        version: "0.3.1",
        new: &[
            "New installations are counted once, anonymously (system, architecture and version only); SOZO_NO_TELEMETRY=1 turns it off",
        ],
        fixed: &[],
    },
    Notes {
        version: "0.3.0",
        new: &[
            "Markdown headings, quotes, rules and code blocks are drawn; hover a line for its source",
            "Mermaid diagrams in Markdown are drawn with box characters",
            "Tables draw bold, italic and highlights inside their cells",
            "Each side of a split editor keeps its own tabs; drag tabs back to join them",
            "Long lines scroll sideways with Shift and the mouse wheel",
            "The tab bar scrolls with the mouse wheel or its < and > arrows",
        ],
        fixed: &["Linux: the right terminal now opens with Ctrl+Alt+B"],
    },
    Notes {
        version: "0.2.0",
        new: &[
            "Files show an icon for their type in the Explorer and the tabs",
            "Richer syntax colours: keywords, types, functions, strings and numbers",
            "Drag a tab to an edge of the editor to split it side by side or stacked",
            "Markdown draws quotes, *italic*, **bold** and ==highlight==",
            "Shortcuts are shown the way your system types them",
            "A short tutorial on the first start, and these notes after each update",
            "About: the flower grows, and a button opens the repository to contribute",
        ],
        fixed: &[
            "The mouse wheel scrolls the editor without jumping back to the cursor",
            "Search results open at the line they found",
            "The empty editor stays centred when panels open or close",
        ],
    },
];
