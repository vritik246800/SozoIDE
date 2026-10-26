//! What this version brings, shown once after an update (F18). Rewritten with every release:
//! a test keeps `version` equal to `Cargo.toml`.

pub struct Notes {
    pub version: &'static str,
    pub new: &'static [&'static str],
    pub fixed: &'static [&'static str],
}

pub const NOTES: Notes = Notes {
    version: "0.3.3",
    new: &[
        "Updates always install the latest release, even when a newer one comes out while SozoIDE is open",
    ],
    fixed: &[],
};
