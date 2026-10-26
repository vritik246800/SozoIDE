//! What this version brings, shown once after an update (F18). Rewritten with every release:
//! a test keeps `version` equal to `Cargo.toml`.

pub struct Notes {
    pub version: &'static str,
    pub new: &'static [&'static str],
    pub fixed: &'static [&'static str],
}

pub const NOTES: Notes = Notes {
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
};
