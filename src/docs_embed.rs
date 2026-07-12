//! docs_embed — the documentation, EMBEDDED in the binary.
//!
//! The GUI's Docs index (`/api/docs`, `/api/doc`) read `docs/*.md` from disk. That works on
//! a workstation checkout, but on Android the app ships only the executable — there is no
//! `docs/` directory beside it, so the Docs feature was silently empty on the phone. The
//! same fix as the GUI pages (src/site.rs): the docs travel inside the binary, and the
//! server prefers a real `docs/` directory when present (so editing a page and reloading
//! still works in development), falling back to these embedded copies otherwise.

/// One embedded doc: stem (no extension) and its markdown bytes.
pub struct Doc {
    pub stem: &'static str,
    pub bytes: &'static [u8],
}

pub const DOCS: &[Doc] = &[
    Doc { stem: "accountable-planning", bytes: include_bytes!("../docs/accountable-planning.md") },
    Doc { stem: "adding-libraries", bytes: include_bytes!("../docs/adding-libraries.md") },
    Doc { stem: "android", bytes: include_bytes!("../docs/android.md") },
    Doc { stem: "building-and-running", bytes: include_bytes!("../docs/building-and-running.md") },
    Doc { stem: "collaborative-notes", bytes: include_bytes!("../docs/collaborative-notes.md") },
    Doc { stem: "conlang-tools", bytes: include_bytes!("../docs/conlang-tools.md") },
    Doc { stem: "data-intensive", bytes: include_bytes!("../docs/data-intensive.md") },
    Doc { stem: "distributed-execution", bytes: include_bytes!("../docs/distributed-execution.md") },
    Doc { stem: "environment", bytes: include_bytes!("../docs/environment.md") },
    Doc { stem: "facet-language", bytes: include_bytes!("../docs/facet-language.md") },
    Doc { stem: "interaction-nets", bytes: include_bytes!("../docs/interaction-nets.md") },
    Doc { stem: "interview-techniques", bytes: include_bytes!("../docs/interview-techniques.md") },
    Doc { stem: "latte-language", bytes: include_bytes!("../docs/latte-language.md") },
    Doc { stem: "latte-tutorial", bytes: include_bytes!("../docs/latte-tutorial.md") },
    Doc { stem: "network-gui", bytes: include_bytes!("../docs/network-gui.md") },
    Doc { stem: "newswire", bytes: include_bytes!("../docs/newswire.md") },
    Doc { stem: "planning", bytes: include_bytes!("../docs/planning.md") },
    Doc { stem: "scars-sound-changes", bytes: include_bytes!("../docs/scars-sound-changes.md") },
    Doc { stem: "security", bytes: include_bytes!("../docs/security.md") },
    Doc { stem: "the-system", bytes: include_bytes!("../docs/the-system.md") },
    Doc { stem: "using-latte-from-the-gui", bytes: include_bytes!("../docs/using-latte-from-the-gui.md") },
    Doc { stem: "visualization-and-ml", bytes: include_bytes!("../docs/visualization-and-ml.md") },
];

/// The markdown bytes for a doc by stem, or None.
pub fn get(stem: &str) -> Option<&'static [u8]> {
    DOCS.iter().find(|d| d.stem == stem).map(|d| d.bytes)
}

/// Every embedded doc stem (sorted).
pub fn stems() -> Vec<&'static str> {
    DOCS.iter().map(|d| d.stem).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_docs_are_embedded() {
        for s in ["security", "environment", "newswire", "android", "building-and-running"] {
            assert!(get(s).is_some(), "missing embedded doc: {}", s);
        }
    }

    #[test]
    fn embedded_docs_match_disk_when_present() {
        for d in DOCS {
            let p = std::path::Path::new("docs").join(format!("{}.md", d.stem));
            if let Ok(disk) = std::fs::read(&p) {
                assert_eq!(disk, d.bytes, "embedded copy of {}.md is stale", d.stem);
            }
        }
    }
}
