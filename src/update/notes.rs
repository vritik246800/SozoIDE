//! What this version brings, shown once after an update (F18). Rewritten with every release:
//! a test keeps `version` equal to `Cargo.toml`.

pub struct Notes {
    pub version: &'static str,
    pub new: &'static [&'static str],
    pub fixed: &'static [&'static str],
}

pub const NOTES: Notes = Notes {
    version: "0.3.1",
    new: &[
        "Installations and updates are counted once each, anonymously (system, architecture and version only); SOZO_NO_TELEMETRY=1 turns it off",
    ],
    fixed: &[],
};
