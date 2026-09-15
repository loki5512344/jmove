//! Move planning: decide which import specifiers must be rewritten.
//!
//! A plan is pure data (no disk writes), so dry-run and `--json` can render
//! it without touching the filesystem. Specifier arithmetic lives in
//! [`specifier`]; the Java package/directory flavour in [`java`], and the
//! mirrored batch move of a whole directory in [`dir`].

mod dir;
mod java;
#[cfg(test)]
pub(crate) use dir::tests_support;
mod specifier;

pub use specifier::relative_specifier;

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::core::index::Index;
use crate::core::{Edit, JmoveError, JmoveResult, normalize_rel_path, rel_str};
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

impl From<&Rewrite> for Edit {
    fn from(rewrite: &Rewrite) -> Self {
        Edit {
            span: rewrite.span.clone(),
            old_text: rewrite.old_text.clone(),
            new_text: rewrite.new_text.clone(),
        }
    }
}

/// One physical file relocation inside a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMove {
    /// Project-relative file being moved.
    pub source: PathBuf,
    /// Project-relative destination path.
    pub target: PathBuf,
}

/// Complete plan for moving `source` to `target`: a file move produces one
/// [`FileMove`], a directory move one per indexed member (see [`dir`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlan {
    /// Requested source: the file, or the directory whose members move.
    pub source: PathBuf,
    /// Requested destination.
    pub target: PathBuf,
    /// Every physical relocation, in sorted source order.
    pub moves: Vec<FileMove>,
    /// Specifier/package rewrites, merged across moves, sorted by (file, span).
    pub rewrites: Vec<Rewrite>,
    /// Directory moves only: real files under `source` that no plan step
    /// moves (unindexable assets) — reported, never silently relocated.
    pub left_behind: Vec<PathBuf>,
    /// Directory moves only: source directories to prune (deepest first)
    /// once every move landed. Removal only succeeds when a directory is
    /// empty, so `left_behind` files keep their home in place — exactly right.
    pub prune_dirs: Vec<PathBuf>,
}

/// Compute the rewrite plan for `source -> target`; a `source` that is a
/// directory on disk becomes a mirrored move of all indexed members below it.
///
/// TS/JS: every indexed import whose resolved target equals a moved file gets
/// a new relative specifier from the importer's directory to its destination
/// (see [`relative_specifier`]). Java moves additionally rewrite the moved
/// file's `package` declaration (see [`java`]). Rewrites whose result equals
/// the old specifier are dropped; the result is sorted by (file, span).
pub fn plan_move(index: &Index, source: &Path, target: &Path) -> JmoveResult<MovePlan> {
    let rel = |label: &str, p: &Path| match normalize_rel_path(p) {
        Some(r) => Ok(r),
        None => Err(JmoveError::InvalidArgument(format!(
            "invalid {label} '{}'",
            p.display()
        ))),
    };
    let (source, target) = (rel("source path", source)?, rel("target path", target)?);
    if source == target {
        return Err(JmoveError::PlanRejected(
            "source and target are the same".into(),
        ));
    }
    // A directory on disk turns the command into a mirrored batch move.
    if index.root.join(&source).is_dir() {
        return dir::plan_dir(index, &source, &target);
    }
    if !index.files.contains(&source) {
        let s = rel_str(&source);
        return Err(JmoveError::InvalidArgument(format!(
            "source '{s}' is not an indexed file"
        )));
    }
    if index.files.contains(&target) {
        let t = rel_str(&target);
        return Err(JmoveError::PlanRejected(format!(
            "target '{t}' already exists"
        )));
    }
    let rewrites = file_rewrites(index, &source, &target)?;
    Ok(MovePlan {
        source: source.clone(),
        target: target.clone(),
        moves: vec![FileMove { source, target }],
        rewrites,
        left_behind: Vec::new(),
        prune_dirs: Vec::new(),
    })
}

/// The per-file rewrite set: Java moves rewrite the `package` declaration
/// too; shared by single-file and directory planning.
pub(super) fn file_rewrites(
    index: &Index,
    source: &Path,
    target: &Path,
) -> JmoveResult<Vec<Rewrite>> {
    Ok(
        if SourceLanguage::for_path(source) == Some(SourceLanguage::Java) {
            java::java_rewrites(index, source, target)?
        } else {
            ts_rewrites(index, source, target)
        },
    )
}

// Relative-specifier rewrites for the TS/JS flavour of the graph.
fn ts_rewrites(index: &Index, source: &Path, target: &Path) -> Vec<Rewrite> {
    let mut rewrites = Vec::new();
    for importer in index.importers_of(source) {
        let edges = index.imports[&importer]
            .iter()
            .filter(|e| e.target.as_deref() == Some(source));
        for edge in edges {
            // Aliased imports keep their alias shape when the new location
            // still round-trips through the same mapping; everything else
            // gets the relative rewrite.
            let new_text = index
                .aliases
                .remap(&edge.record.specifier, target, &index.files)
                .unwrap_or_else(|| relative_specifier(&importer, target));
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
    use super::tests_support::edge;
    use super::{Rewrite, plan_move};
    use crate::core::JmoveError;
    use crate::core::index::{Index, ResolvedImport};
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
            .map(|r| (crate::core::rel_str(&r.file), r.span.start))
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
        assert_eq!(plan.moves.len(), 1);
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
