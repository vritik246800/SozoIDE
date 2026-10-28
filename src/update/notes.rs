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
