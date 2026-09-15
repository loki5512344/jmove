//! Module specifier resolution for TS/JS projects.
//!
//! CONTRACT: see [`crate::parser`]. Given a *relative* specifier and the
//! importing file, find which indexed project file it refers to. Bare /
//! package specifiers (not starting with `.`) are out of project scope and
//! resolve to `None`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::core::index::FileSet;
use crate::core::normalize_rel_path;

/// Supported extensions, in Node/TS resolution priority order.
const EXTENSIONS: [&str; 6] = ["ts", "tsx", "js", "jsx", "mjs", "cjs"];

/// Module extension *suffixes*, longest-first: `resolve_base` re-adds them
/// to a specifier, alias remapping sheds them again (the exact inverse).
pub const MODULE_EXTS: &[&str] = &[".d.ts", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];

/// Ambient declaration files: only consulted after every real module
/// candidate missed (last resort).
const DECLARATION_EXT: &str = "d.ts";

/// Append `.{ext}` to a path without touching its existing extension
/// (specifiers are joined verbatim, matching Node's lookup).
fn with_ext(base: &Path, ext: &str) -> PathBuf {
    let mut joined = OsString::from(base.as_os_str());
    joined.push(".");
    joined.push(ext);
    PathBuf::from(joined)
}

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
    if !specifier.starts_with('.') {
        return None; // bare package specifier: outside project scope
    }
    // `importer` is project-relative, so `..` segments that walk past the
    // root collapse to `None` here instead of escaping the index.
    let base = normalize_rel_path(&importer.parent()?.join(specifier))?;
    resolve_base(&base, files)
}

/// Resolve a project-relative module base path against the file set:
/// exact file, extension guessing, then `index.*` in the directory.
/// Shared by relative specifiers and tsconfig alias mapping.
#[must_use]
pub fn resolve_base(base: &Path, files: &FileSet) -> Option<PathBuf> {
    if files.contains(base) {
        return Some(base.to_path_buf());
    }
    let base = base.to_path_buf();
    for ext in EXTENSIONS {
        let candidate = with_ext(&base, ext);
        if files.contains(&candidate) {
            return Some(candidate);
        }
    }
    let declaration = with_ext(&base, DECLARATION_EXT);
    if files.contains(&declaration) {
        return Some(declaration);
    }
    let index_dir = base.join("index");
    EXTENSIONS
        .iter()
        .map(|ext| with_ext(&index_dir, ext))
        .find(|candidate| files.contains(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> FileSet {
        let mut set = FileSet::default();
        for path in paths {
            set.add(PathBuf::from(path));
        }
        set
    }

    #[track_caller]
    fn resolve(importer: &str, specifier: &str, set: &FileSet) -> Option<PathBuf> {
        resolve_module(Path::new(importer), specifier, set)
    }

    #[test]
    fn exact_specifier_with_extension() {
        let set = files(&["src/a.ts"]);
        assert_eq!(
            resolve("src/b.ts", "./a.ts", &set),
            Some(PathBuf::from("src/a.ts"))
        );
    }

    #[test]
    fn guesses_extensions_in_priority_order() {
        let set = files(&["src/a.ts", "src/a.js"]);
        assert_eq!(
            resolve("src/b.ts", "./a", &set),
            Some(PathBuf::from("src/a.ts"))
        );
        let only_js = files(&["src/a.js"]);
        assert_eq!(
            resolve("src/b.ts", "./a", &only_js),
            Some(PathBuf::from("src/a.js"))
        );
    }

    #[test]
    fn jsx_and_component_specifiers() {
        let set = files(&["src/Comp.tsx"]);
        assert_eq!(
            resolve("src/app.tsx", "./Comp", &set),
            Some(PathBuf::from("src/Comp.tsx"))
        );
    }

    #[test]
    fn declaration_file_is_last_resort() {
        let set = files(&["types/globals.d.ts"]);
        assert_eq!(
            resolve("src/a.ts", "../types/globals", &set),
            Some(PathBuf::from("types/globals.d.ts"))
        );
        // a real module wins over the declaration file
        let both = files(&["types/globals.d.ts", "types/globals.ts"]);
        assert_eq!(
            resolve("src/a.ts", "../types/globals", &both),
            Some(PathBuf::from("types/globals.ts"))
        );
    }

    #[test]
    fn directory_index_resolution() {
        let set = files(&["pkg/index.ts", "other/index.js", "other/util.ts"]);
        assert_eq!(
            resolve("src/a.ts", "../pkg", &set),
            Some(PathBuf::from("pkg/index.ts"))
        );
        assert_eq!(resolve("src/a.ts", "./", &set), None);
        assert_eq!(
            resolve("src/a.ts", "../other", &set),
            Some(PathBuf::from("other/index.js"))
        );
    }

    #[test]
    fn bare_specifiers_are_not_project_paths() {
        let set = files(&["react.ts", "lodash.ts"]);
        assert_eq!(resolve("src/a.ts", "react", &set), None);
        assert_eq!(resolve("src/a.ts", "@scope/pkg", &set), None);
        assert_eq!(resolve("src/a.ts", "/abs/path", &set), None);
    }

    #[test]
    fn escaping_the_project_root_resolves_to_none() {
        let set = files(&["evil/x.ts", "outside.ts"]);
        assert_eq!(resolve("src/a.ts", "../../evil/x", &set), None);
        assert_eq!(resolve("a.ts", "../outside", &set), None);
    }

    #[test]
    fn nested_parent_walks_land_inside_the_project() {
        let set = files(&["lib/x.ts", "src/app/main.ts"]);
        assert_eq!(
            resolve("src/app/main.ts", "../../lib/x", &set),
            Some(PathBuf::from("lib/x.ts"))
        );
    }

    #[test]
    fn forward_slash_specifiers_and_redundant_dots() {
        let set = files(&["src/nested/dir/deep.ts"]);
        assert_eq!(
            resolve("src/app.ts", "./nested/./dir/deep", &set),
            Some(PathBuf::from("src/nested/dir/deep.ts"))
        );
    }

    #[test]
    fn unresolvable_relative_specifier_is_none() {
        let set = files(&["src/a.ts"]);
        assert_eq!(resolve("src/a.ts", "./missing", &set), None);
        assert_eq!(resolve("src/a.ts", "./styles.css", &set), None);
    }

    #[test]
    fn root_level_importer_resolves_plain_sibling() {
        let set = files(&["index.ts", "b.ts"]);
        assert_eq!(
            resolve("index.ts", "./b", &set),
            Some(PathBuf::from("b.ts"))
        );
    }
}
