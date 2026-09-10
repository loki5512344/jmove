//! Atomic apply with rollback, plus unified-diff rendering for dry-run.
//!
//! Rewrites land on importer files first (each atomically via temp-file +
//! rename), the `source -> target` rename happens last, and any failure
//! mid-way rolls back everything already written.

mod diff;
mod fsops;

pub use diff::render_diff;

use std::fs;
use std::path::{Path, PathBuf};

use crate::core::apply::fsops::{
    create_missing_dirs, group_by_file, rewrite_bytes, sibling_temp, write_durable,
};
use crate::core::plan::{MovePlan, Rewrite};
use crate::core::{JmoveError, JmoveResult};

/// Summary of a successfully applied plan.
#[derive(Debug, Clone)]
pub struct Applied {
    /// Number of files whose imports were rewritten.
    pub files_rewritten: usize,
    /// The moved file's new project-relative path.
    pub new_path: PathBuf,
}

// Rollback state for one run: originals of rewritten files (newest last),
// dirs created for the target, and the final rename once it happened.
#[derive(Default)]
struct Run {
    root: PathBuf,
    backups: Vec<(PathBuf, Vec<u8>)>,
    dirs: Vec<PathBuf>,
    moved: Option<(PathBuf, PathBuf)>,
}

/// Apply `plan` under `root` atomically (see module docs). Rollback is
/// best-effort: on restore failure the error names the files that need
/// manual recovery.
pub fn apply(root: &Path, plan: &MovePlan) -> JmoveResult<Applied> {
    let mut run = Run {
        root: root.to_path_buf(),
        ..Default::default()
    };
    match run.try_apply(plan) {
        Ok(applied) => Ok(applied),
        Err(err) => Err(run.undo(err)),
    }
}

impl Run {
    fn try_apply(&mut self, plan: &MovePlan) -> JmoveResult<Applied> {
        let by_file = group_by_file(plan);
        for (file, rewrites) in &by_file {
            self.rewrite_one(file, rewrites)?;
        }
        // The move comes last, after every importer was rewritten.
        let (src, dst) = (self.root.join(&plan.source), self.root.join(&plan.target));
        self.dirs = create_missing_dirs(&dst)?;
        fs::rename(&src, &dst)?;
        self.moved = Some((src, dst));
        Ok(Applied {
            files_rewritten: by_file.len(),
            new_path: plan.target.clone(),
        })
    }

    // Patch one file in memory, then temp-file + fsync + rename over it;
    // the original bytes go to `backups` for rollback.
    fn rewrite_one(&mut self, file: &Path, rewrites: &[&Rewrite]) -> JmoveResult<()> {
        let path = self.root.join(file);
        let original = fs::read(&path)?;
        let patched = rewrite_bytes(&original, rewrites)?;
        self.backups.push((file.to_path_buf(), original));
        let temp = sibling_temp(&path); // same dir => rename stays atomic
        write_durable(&temp, &patched)?;
        if let Err(err) = fs::rename(&temp, &path) {
            let _ = fs::remove_file(&temp); // no stray temp behind
            return Err(err.into());
        }
        Ok(())
    }

    // Undo newest-first; keep the original error, appending any rollback
    // problems to its message.
    fn undo(&mut self, err: JmoveError) -> JmoveError {
        let mut problems = Vec::new();
        if let Some((src, dst)) = self.moved.take()
            && let Err(e) = fs::rename(&dst, &src)
        {
            problems.push(format!("could not move back {}: {e}", dst.display()));
        }
        for (file, bytes) in self.backups.drain(..).rev() {
            if let Err(e) = fs::write(self.root.join(&file), &bytes) {
                problems.push(format!("could not restore {}: {e}", file.display()));
            }
        }
        for dir in self.dirs.drain(..).rev() {
            let _ = fs::remove_dir(&dir); // best-effort: only empty dirs
        }
        if problems.is_empty() {
            return err;
        }
        let msg = format!("{err}; rollback incomplete: {}", problems.join("; "));
        JmoveError::Io(std::io::Error::other(msg))
    }
}

#[cfg(test)]
mod tests {
    use super::apply;
    use crate::core::JmoveResult;
    use crate::core::plan::{MovePlan, Rewrite};
    use std::fs;
    use std::path::{Path, PathBuf};

    const OLD: &str = "import {\n  fmt,\n} from '../lib/fmt';\n";
    const NEW: &str = "import {\n  fmt,\n} from '../deep/fmt';\n";
    // Byte span of `../lib/fmt` (between the quotes) inside OLD.
    const SPAN: std::ops::Range<usize> = 24..34;

    // Plan moving lib/fmt.ts -> deep/fmt.ts, rewriting src/app.ts.
    fn plan() -> MovePlan {
        let rewrite = Rewrite {
            file: "src/app.ts".into(),
            span: SPAN,
            old_text: "../lib/fmt".into(),
            new_text: "../deep/fmt".into(),
        };
        MovePlan {
            source: "lib/fmt.ts".into(),
            target: "deep/fmt.ts".into(),
            rewrites: vec![rewrite],
        }
    }

    fn mk(dir: &Path, rel: &str, body: &str) -> JmoveResult<()> {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, body)?;
        Ok(())
    }

    #[test]
    fn apply_rewrites_spans_creates_dirs_and_moves_last() -> JmoveResult<()> {
        assert_eq!(&OLD[SPAN], "../lib/fmt"); // sanity: the span is real
        let dir = tempfile::TempDir::new()?;
        let root = dir.path();
        mk(root, "src/app.ts", OLD)?;
        mk(root, "lib/fmt.ts", "export const fmt = 1;\n")?;
        let applied = apply(root, &plan())?;
        assert_eq!(
            (applied.files_rewritten, &applied.new_path),
            (1, &PathBuf::from("deep/fmt.ts"))
        );
        assert!(!root.join("lib/fmt.ts").exists());
        assert_eq!(
            fs::read_to_string(root.join("deep/fmt.ts"))?,
            "export const fmt = 1;\n"
        );
        // Only the specifier bytes changed; the layout is kept byte-exact.
        assert_eq!(fs::read_to_string(root.join("src/app.ts"))?, NEW);
        assert!(!root.join("src/app.ts.jmove-tmp").exists());
        Ok(())
    }

    #[test]
    fn apply_rolls_back_when_the_move_fails() -> JmoveResult<()> {
        // Missing source: the last rename fails after the rewrite landed.
        let dir = tempfile::TempDir::new()?;
        mk(dir.path(), "src/app.ts", OLD)?;
        let err = apply(dir.path(), &plan()).expect_err("missing source");
        assert!(matches!(err, crate::core::JmoveError::Io(_)), "{err}");
        // Importer restored to its exact original bytes; created dirs gone.
        assert_eq!(fs::read_to_string(dir.path().join("src/app.ts"))?, OLD);
        assert!(!dir.path().join("deep").exists());
        Ok(())
    }

    #[test]
    fn apply_rejects_a_stale_plan_without_writing() -> JmoveResult<()> {
        // SPAN was computed on OLD's layout; a single-line importer has
        // different bytes there, so the run fails before any write.
        let other = "import { fmt } from '../lib/fmt';\n";
        let dir = tempfile::TempDir::new()?;
        let root = dir.path();
        mk(root, "src/app.ts", other)?;
        mk(root, "lib/fmt.ts", "export const fmt = 1;\n")?;
        let err = apply(root, &plan()).expect_err("span mismatch");
        assert!(
            matches!(err, crate::core::JmoveError::StaleIndex(_)),
            "{err}"
        );
        assert_eq!(fs::read_to_string(root.join("src/app.ts"))?, other);
        assert!(root.join("lib/fmt.ts").exists());
        Ok(())
    }
}
