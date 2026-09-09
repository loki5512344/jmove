//! Module specifier resolution for TS/JS projects.
//!
//! CONTRACT: see [`crate::parser`]. Given a *relative* specifier and the
//! importing file, find which indexed project file it refers to. Bare /
//! package specifiers (not starting with `.`) are out of project scope and
//! resolve to `None`.

use std::path::{Path, PathBuf};

use crate::core::index::FileSet;

/// Resolve `specifier` (e.g. `"../utils/fmt"`) written in the file at
/// `importer` (project-relative), against the indexed `files`.
///
/// Resolution order for extensionless specifiers (Node/TS classic):
/// 1. exact path if indexed (e.g. `"./a.ts"`),
/// 2. `<base>` + each supported extension (`.ts`, `.tsx`, `.js`, `.jsx`,
///    `.mjs`, `.cjs` — declaration files only when nothing else matches),
/// 3. `<base>/index.<ext>`.
///
/// Returns `None` for bare specifiers or unresolvable paths.
#[must_use]
pub fn resolve_module(importer: &Path, specifier: &str, files: &FileSet) -> Option<PathBuf> {
    let _ = (importer, specifier, files);
    todo!("parser agent: implement resolver")
}
