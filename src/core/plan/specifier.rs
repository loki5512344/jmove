// Pure specifier arithmetic: TS module-stem conventions and directory
// component math. Separated from planner logic to keep plan/ files small.

use std::path::{Component, Path};

// Module stem of a file name: `x.d.ts` -> `x`, `x.ts` -> `x`, unknown or
// missing extension kept. `index` is never stripped (stays conservative).
fn module_stem(name: &str) -> &str {
    if let Some(s) = name.strip_suffix(".d.ts") {
        return s;
    }
    match name.rsplit_once('.') {
        Some((s, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs")) => s,
        _ => name,
    }
}

// Normal directory components of a project-relative path.
fn dir_parts(path: &Path) -> Vec<String> {
    let parent = path.parent().unwrap_or(Path::new(""));
    parent
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// Compute the TS/JS relative specifier from `importer`'s directory to
/// `target`, prefixed with `./` or `../` as needed. Extension stripping
/// follows TS module-resolution convention: `.ts`/`.tsx`/`.js`/`.jsx`/
/// `.mjs`/`.cjs` are removed from file targets (an importer that wrote the
/// extension may keep doing so — the specifier stays valid).
///
/// ```
/// # use std::path::Path;
/// # use jmove::core::plan::relative_specifier;
/// assert_eq!(relative_specifier(Path::new("src/services/a.ts"), Path::new("src/utils/fmt.ts")), "../utils/fmt");
/// ```
#[must_use]
pub fn relative_specifier(importer: &Path, target: &Path) -> String {
    // Component math: consume the common directory prefix, one `..` per
    // leftover importer dir; the stem joins last, never as a directory.
    let name = target
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let (stem, dirs, mdirs) = (module_stem(&name), dir_parts(importer), dir_parts(target));
    let common = dirs.iter().zip(&mdirs).take_while(|(a, b)| a == b).count();
    let up = dirs.len() - common;
    let mut parts = vec!["..".to_string(); up];
    parts.extend_from_slice(&mdirs[common..]);
    parts.push(stem.to_string());
    let joined = parts.join("/");
    if up > 0 {
        joined // already starts with `../`
    } else {
        format!("./{joined}")
    }
}

#[cfg(test)]
mod tests {
    use super::relative_specifier;
    use std::path::Path;

    #[test]
    fn relative_specifier_table() {
        let cases = [
            ("src/a.ts", "src/b.ts", "./b"),                      // sibling
            ("src/a.ts", "src/lib/b.ts", "./lib/b"),              // child
            ("src/lib/a.ts", "src/b.ts", "../b"),                 // parent
            ("a/b/c/x.ts", "root/file.ts", "../../../root/file"), // `..` chain
            ("main.ts", "src/util.ts", "./src/util"),             // root importer
            ("src/main.ts", "util.js", "../util"),                // root target
            ("src/a.ts", "src/foo/index.ts", "./foo/index"),      // index kept
            ("src/a/deep.ts", "src/foo/index.ts", "../foo/index"),
            ("src/a.ts", "src/shim.d.ts", "./shim"), // .d.ts dropped
            ("src/a.ts", "types/global.d.ts", "../types/global"),
            ("src/a.ts", "src/b.mts", "./b.mts"), // unknown ext kept
            ("src/a.ts", "data/config.json", "../data/config.json"),
        ];
        for (i, t, want) in cases {
            assert_eq!(
                relative_specifier(Path::new(i), Path::new(t)),
                want,
                "{i} -> {t}"
            );
        }
    }
}
