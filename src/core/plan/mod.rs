//! Move planning: decide which import specifiers must be rewritten.
//!
//! A plan is pure data (no disk writes), so dry-run and `--json` can render
//! it without touching the filesystem. Specifier arithmetic lives in
//! [`specifier`]; the Java package/directory flavour in [`java`].

mod java;
mod specifier;
#[cfg(test)]
pub(crate) mod tests_support;

pub use specifier::relative_specifier;

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::core::index::Index;
use crate::core::{JmoveError, JmoveResult, normalize_rel_path};
use crate::parser::SourceLanguage;

/// One in-file replacement of an import specifier. Only the specifier text
/// between the quotes is touched — the statement layout is never reformatted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    /// Project-relative file to modify.
    pub file: PathBuf,
    /// Byte range of the old specifier text (without quotes).
    pub span: Range<usize>,
    /// Specifier as currently written.
    pub old_text: String,
    /// Specifier after the move.
    pub new_text: String,
}

/// Complete plan for moving `source` to `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlan {
    /// Project-relative path being moved.
    pub source: PathBuf,
    /// Project-relative destination path.
    pub target: PathBuf,
    /// Specifier rewrites, sorted by (file, span).
    pub rewrites: Vec<Rewrite>,
}

/// Compute the rewrite plan for `source -> target`.
///
/// TS/JS: every indexed import whose resolved target equals `source` gets a
/// new relative specifier from the importer's directory to `target` (see
/// [`relative_specifier`]). Java moves additionally rewrite the moved file's
/// `package` declaration (see [`java`]). Rewrites whose result equals the
/// old specifier are dropped; the result is sorted by (file, span).
pub fn plan_move(index: &Index, source: &Path, target: &Path) -> JmoveResult<MovePlan> {
    let rel = |label: &str, p: &Path| match normalize_rel_path(p) {
        Some(r) => Ok(r),
        None => Err(JmoveError::InvalidArgument(format!(
            "invalid {label} '{}'",
            p.display()
        ))),
    };
    let (source, target) = (rel("source path", source)?, rel("target path", target)?);
    if !index.files.contains(&source) {
        let s = source.display();
        return Err(JmoveError::InvalidArgument(format!(
            "source '{s}' is not an indexed file"
        )));
    }
    if source == target {
        return Err(JmoveError::PlanRejected(
            "source and target are the same".into(),
        ));
    }
    if index.files.contains(&target) {
        let t = target.display();
        return Err(JmoveError::PlanRejected(format!(
            "target '{t}' already exists"
        )));
    }

    let rewrites = if SourceLanguage::for_path(&source) == Some(SourceLanguage::Java) {
        java::java_rewrites(index, &source, &target)?
    } else {
        ts_rewrites(index, &source, &target)
    };
    Ok(MovePlan {
        source,
        target,
        rewrites,
    })
}

// Relative-specifier rewrites for the TS/JS flavour of the graph.
fn ts_rewrites(index: &Index, source: &Path, target: &Path) -> Vec<Rewrite> {
    let mut rewrites = Vec::new();
    for importer in index.importers_of(source) {
        let edges = index.imports[&importer]
            .iter()
            .filter(|e| e.target.as_deref() == Some(source));
        for edge in edges {
            let new_text = relative_specifier(&importer, target);
            if new_text == edge.record.specifier {
                continue; // no-op rewrite, never reaches the plan
            }
            let record = &edge.record;
            rewrites.push(Rewrite {
                file: importer.clone(),
                span: record.span.clone(),
                old_text: record.specifier.clone(),
                new_text,
            });
        }
    }
    rewrites.sort_by_key(|r| (r.file.clone(), r.span.start));
    rewrites
}

#[cfg(test)]
mod tests {
    use super::{Rewrite, plan_move};
    use crate::core::JmoveError;
    use crate::core::index::{Index, ResolvedImport};
    use crate::core::plan::tests_support::edge;
    use std::path::{Path, PathBuf};

    fn index_with(files: &[&str], imports: &[(&str, Vec<ResolvedImport>)]) -> Index {
        let mut ix = Index::default();
        for f in files {
            ix.files.add(PathBuf::from(f));
        }
        ix.imports
            .extend(imports.iter().map(|(f, r)| (PathBuf::from(*f), r.clone())));
        ix
    }

    #[test]
    fn plan_rewrites_sorted_by_file_then_span() {
        let z = vec![edge("./s", 30..33, "s.ts"), edge("./s", 5..8, "s.ts")];
        let a = vec![edge("./s", 0..3, "s.ts")];
        let index = index_with(&["s.ts", "z.ts", "a.ts"], &[("z.ts", z), ("a.ts", a)]);
        let plan = plan_move(&index, Path::new("s.ts"), Path::new("sub/deep/s.ts")).unwrap();
        let keys: Vec<(String, usize)> = plan
            .rewrites
            .iter()
            .map(|r| (r.file.display().to_string(), r.span.start))
            .collect();
        assert_eq!(
            keys,
            [
                ("a.ts".to_string(), 0usize),
                ("z.ts".into(), 5),
                ("z.ts".into(), 30)
            ]
        );
        assert_eq!(
            plan.rewrites[0],
            Rewrite {
                file: "a.ts".into(),
                span: 0..3,
                old_text: "./s".into(),
                new_text: "./sub/deep/s".into(),
            }
        );
    }

    #[test]
    fn plan_drops_noop_rewrites() {
        // fmt.ts -> fmt.js keeps `./lib/fmt` valid: no specifier edits.
        let edges = vec![edge("./lib/fmt", 0..0, "lib/fmt.ts")];
        let index = index_with(&["lib/fmt.ts", "app.ts"], &[("app.ts", edges)]);
        let plan = plan_move(&index, Path::new("lib/fmt.ts"), Path::new("lib/fmt.js")).unwrap();
        assert!(plan.rewrites.is_empty());
        assert_eq!(plan.target, PathBuf::from("lib/fmt.js"));
    }

    #[test]
    fn plan_rejects_bad_source_and_target() {
        let edges = vec![edge("./old", 7..12, "src/old.ts")];
        let index = index_with(&["src/old.ts", "src/app.ts"], &[("src/app.ts", edges)]);
        // A missing source is an invalid argument...
        let err = plan_move(&index, Path::new("ghost.ts"), Path::new("x.ts")).unwrap_err();
        assert!(matches!(err, JmoveError::InvalidArgument(_)));
        // ...an existing or identical target is a plan rejection.
        for target in ["src/app.ts", "src/old.ts"] {
            let err = plan_move(&index, Path::new("src/old.ts"), Path::new(target)).unwrap_err();
            assert!(matches!(err, JmoveError::PlanRejected(_)), "{target}");
        }
    }
}
