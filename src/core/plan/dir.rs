//! Directory moves: relocate every indexed file under `source` into the
//! mirrored layout under `target`, merging the per-file rewrite plans.
//!
//! A directory move is exactly N file moves that must apply atomically
//! together, so it reuses the per-file planner and folds its rewrites;
//! a file that imports two moved classes from the same directory simply
//! gets both specifier edits. Files that exist on disk but are not
//! indexable (assets, binaries) cannot be rewritten and would silently
//! stay behind — they are reported as `left_behind` instead of being
//! moved blindly.

use std::path::{Path, PathBuf};

use super::{FileMove, MovePlan, Rewrite};
use crate::core::index::Index;
use crate::core::{JmoveError, JmoveResult, rel_str};

pub(super) fn plan_dir(index: &Index, source: &Path, target: &Path) -> JmoveResult<MovePlan> {
    let rejected = |msg: String| JmoveError::PlanRejected(format!("Directory move: {msg}"));
    if source.starts_with(target) || target.starts_with(source) {
        return Err(rejected(format!(
            "'{}' and '{}' are nested; a directory cannot move into or out of itself",
            rel_str(source),
            rel_str(target)
        )));
    }
    let members: Vec<PathBuf> = index
        .files
        .sorted()
        .into_iter()
        .filter(|f| f.starts_with(source))
        .collect();
    if members.is_empty() {
        let s = rel_str(source);
        return Err(JmoveError::InvalidArgument(format!(
            "source '{s}' has no indexed source files"
        )));
    }
    let moves: Vec<FileMove> = members
        .iter()
        .map(|f| {
            let dst = target.join(f.strip_prefix(source).unwrap_or(f));
            FileMove {
                source: f.clone(),
                target: dst,
            }
        })
        .collect();
    for m in &moves {
        if index.files.contains(&m.target) {
            let t = rel_str(&m.target);
            return Err(rejected(format!("target '{t}' already exists")));
        }
    }
    let mut rewrites: Vec<Rewrite> = moves
        .iter()
        .map(|m| super::file_rewrites(index, &m.source, &m.target))
        .collect::<JmoveResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    rewrites.sort_by_key(|r| (r.file.clone(), r.span.start));
    rewrites.dedup();
    let mut prune_dirs = prune_chain(source, &members);
    prune_dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    Ok(MovePlan {
        source: source.to_path_buf(),
        target: target.to_path_buf(),
        moves,
        rewrites,
        left_behind: unsupported_left_behind(index, source),
        prune_dirs,
    })
}

// Every directory that loses files: the moved members' parent chains up to
// and including `source` itself.
fn prune_chain(source: &Path, members: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for m in members {
        let mut dir = m.parent().unwrap_or(Path::new("")).to_path_buf();
        loop {
            if dir == source {
                if !out.contains(&source.to_path_buf()) {
                    out.push(source.to_path_buf());
                }
                break;
            }
            if !out.contains(&dir) {
                out.push(dir.clone());
            }
            let Some(up) = dir.parent() else { break };
            dir = up.to_path_buf();
        }
    }
    out
}

// Real files anywhere under `source` that the index does not know (and
// therefore this plan will not move). Walked with the same gitignore rules
// as the scanner; hidden paths are nobody's source files and skipped.
fn unsupported_left_behind(index: &Index, source: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let walk = ignore::WalkBuilder::new(index.root.join(source))
        .require_git(false)
        .build();
    for entry in walk.flatten() {
        if entry.path_is_symlink() || !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(stripped) = entry.path().strip_prefix(&index.root) else {
            continue;
        };
        let Some(rel) = crate::core::normalize_rel_path(stripped) else {
            continue;
        };
        let hidden = rel
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
        if !hidden && !index.files.contains(&rel) {
            out.push(rel);
        }
    }
    out.sort();
    out
}

/// Shared `#[cfg(test)]` graph builders for the plan submodules.
#[cfg(test)]
pub(crate) mod tests_support {
    use std::ops::Range;
    use std::path::PathBuf;

    use crate::core::index::ResolvedImport;
    use crate::parser::ImportRecord;

    /// Hand-wired resolved edge: plan tests never touch the parser.
    pub(crate) fn edge(spec: &str, span: Range<usize>, target: &str) -> ResolvedImport {
        let record = ImportRecord {
            specifier: spec.into(),
            span,
            is_dynamic: false,
        };
        ResolvedImport {
            record,
            target: Some(PathBuf::from(target)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::JmoveResult;
    use crate::core::index::Index;
    use crate::core::plan::plan_move;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn project() -> JmoveResult<tempfile::TempDir> {
        let dir = tempfile::TempDir::new()?;
        let root = dir.path().to_path_buf();
        let write = |rel: &str, body: &str| -> JmoveResult<()> {
            let p = root.join(rel);
            fs::create_dir_all(p.parent().unwrap())?;
            fs::write(p, body)?;
            Ok(())
        };
        write("docs/notes.md", "not source\n")?;
        write("lib/a.ts", "export const a = 1;\n")?;
        write("lib/b.ts", "export const b = 2;\n")?;
        write("lib/data.json", "{}\n")?;
        write(
            "app.ts",
            "import { a } from './lib/a';\nimport { b } from './lib/b';\n",
        )?;
        write("deep/c.ts", "import { a } from '../lib/a';\n")?;
        Ok(dir)
    }

    #[test]
    fn dir_plan_moves_every_indexed_file_and_merges_rewrites() -> JmoveResult<()> {
        let dir = project()?;
        let index = Index::build(dir.path())?;
        let plan = plan_move(&index, Path::new("lib"), Path::new("core/lib"))?;
        assert_eq!(plan.prune_dirs, [PathBuf::from("lib")]);
        // rel_str: canonical '/' display, stable across platforms.
        let moves: Vec<(String, String)> = plan
            .moves
            .iter()
            .map(|m| {
                (
                    crate::core::rel_str(&m.source),
                    crate::core::rel_str(&m.target),
                )
            })
            .collect();
        assert_eq!(
            moves,
            [("lib/a.ts", "core/lib/a.ts"), ("lib/b.ts", "core/lib/b.ts"),]
        );
        // app.ts (root) imports both moved modules: two merged specifier
        // edits; deep/c.ts gets its own relative rewrite.
        let app: Vec<_> = plan
            .rewrites
            .iter()
            .filter(|r| r.file == Path::new("app.ts"))
            .map(|r| r.new_text.as_str())
            .collect();
        assert_eq!(app, ["./core/lib/a", "./core/lib/b"]);
        // The unindexed JSON would stay on disk: report, never silently move.
        assert_eq!(plan.left_behind, [PathBuf::from("lib/data.json")]);
        Ok(())
    }

    #[test]
    fn dir_plan_rejects_nested_and_empty_moves() -> JmoveResult<()> {
        let dir = project()?;
        let index = Index::build(dir.path())?;
        let err = plan_move(&index, Path::new("lib"), Path::new("lib/sub")).unwrap_err();
        assert!(
            matches!(err, crate::core::JmoveError::PlanRejected(_)),
            "{err}"
        );
        // `docs` exists but holds no indexable file: nothing to move.
        let err = plan_move(&index, Path::new("docs"), Path::new("core")).unwrap_err();
        assert!(
            matches!(err, crate::core::JmoveError::InvalidArgument(_)),
            "{err}"
        );
        Ok(())
    }
}
