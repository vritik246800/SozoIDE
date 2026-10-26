//! What this version brings, shown once after an update (F18). Rewritten with every release:
//! a test keeps `version` equal to `Cargo.toml`.

pub struct Notes {
    pub version: &'static str,
    pub new: &'static [&'static str],
    pub fixed: &'static [&'static str],
}

pub const NOTES: Notes = Notes {
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
};
