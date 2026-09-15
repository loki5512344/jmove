//! Atomic apply with rollback, plus unified-diff rendering for dry-run.
//!
//! Two entry points share one engine: [`apply`] writes a `mv` plan (its
//! rewrites land on importer files first, each atomically via temp-file +
//! rename, and the `source -> target` rename happens last — through
//! `git mv` for tracked files, see [`GitMode`]), [`apply_edits`] writes
//! plain edit groups without a move (the `fix` flavour). Any failure
//! mid-way rolls back everything already written.

mod diff;
mod fsops;
mod git;

pub use diff::{render_diff, render_edits_diff};
pub use git::{GitMode, would_use_git};

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::Edit;
use crate::core::apply::fsops::{
    create_missing_dirs, group_by_file, rewrite_bytes, sibling_temp, write_durable,
};
use crate::core::plan::{FileMove, MovePlan};
use crate::core::{JmoveError, JmoveResult, rel_str};

/// Summary of a successfully applied plan.
#[derive(Debug, Clone)]
pub struct Applied {
    /// Number of files whose imports were rewritten.
    pub files_rewritten: usize,
    /// The moved file's new project-relative path (directory moves: the
    /// requested destination, mirroring [`crate::core::plan::MovePlan::target`]).
    pub new_path: PathBuf,
    /// Whether every physical rename went through `git mv`.
    pub via_git: bool,
}

// Rollback state for one run: originals of rewritten files (newest last),
// dirs created for the targets, and the renames once they happened.
#[derive(Default)]
struct Run {
    root: PathBuf,
    backups: Vec<(PathBuf, Vec<u8>)>,
    dirs: Vec<PathBuf>,
    // Executed root-relative renames (src, dst, went-through-git), oldest first.
    moved: Vec<(PathBuf, PathBuf, bool)>,
}

/// Apply `plan` under `root` atomically (see module docs). Rollback is
/// best-effort: on restore failure the error names the files that need
/// manual recovery. `git` selects between `git mv` and plain rename.
pub fn apply(root: &Path, plan: &MovePlan, git: GitMode) -> JmoveResult<Applied> {
    let mut run = Run {
        root: root.to_path_buf(),
        ..Default::default()
    };
    match run.try_apply(plan, git) {
        Ok(applied) => Ok(applied),
        Err(err) => Err(run.undo(err)),
    }
}

/// Apply pre-grouped edits under `root` atomically, without any move;
/// returns the number of files written. Same rollback contract as
/// [`apply`].
pub fn apply_edits(root: &Path, by_file: &BTreeMap<PathBuf, Vec<Edit>>) -> JmoveResult<usize> {
    let mut run = Run {
        root: root.to_path_buf(),
        ..Default::default()
    };
    match run.try_fix(by_file) {
        Ok(files) => Ok(files),
        Err(err) => Err(run.undo(err)),
    }
}

impl Run {
    fn try_apply(&mut self, plan: &MovePlan, mode: GitMode) -> JmoveResult<Applied> {
        let by_file = group_by_file(plan);
        self.write_all(&by_file)?;
        // The moves come last, after every importer was rewritten. A
        // directory plan is applied file by file in sorted order; any
        // failure rolls the whole batch back.
        for m in &plan.moves {
            self.move_one(mode, m)?;
        }
        // Directory moves leave their emptied source dirs behind otherwise;
        // remove_dir only succeeds when truly empty, so `left_behind` files
        // keep their home. This is the last step: nothing can fail after it.
        for d in &plan.prune_dirs {
            let _ = fs::remove_dir(self.root.join(d));
        }
        Ok(Applied {
            files_rewritten: by_file.len(),
            new_path: plan.target.clone(),
            via_git: !self.moved.is_empty() && self.moved.iter().all(|(_, _, g)| *g),
        })
    }

    fn move_one(&mut self, mode: GitMode, m: &FileMove) -> JmoveResult<()> {
        let (src, dst) = (self.root.join(&m.source), self.root.join(&m.target));
        for created in create_missing_dirs(&dst)? {
            self.dirs.push(created);
        }
        // git mv needs the destination dir to exist; tracked sources are
        // renamed through git so the change lands staged in the index.
        let via_git = would_use_git(&self.root, &m.source, mode);
        let done = if via_git {
            git::mv(&self.root, &m.source, &m.target)
                .map(|()| (m.source.clone(), m.target.clone(), true))
        } else {
            fs::rename(&src, &dst)
                .map(|()| (m.source.clone(), m.target.clone(), false))
                .map_err(Into::into)
        };
        self.moved.push(done?);
        Ok(())
    }

    fn try_fix(&mut self, by_file: &BTreeMap<PathBuf, Vec<Edit>>) -> JmoveResult<usize> {
        self.write_all(by_file)?;
        Ok(by_file.len())
    }

    fn write_all(&mut self, by_file: &BTreeMap<PathBuf, Vec<Edit>>) -> JmoveResult<()> {
        for (file, edits) in by_file {
            self.rewrite_one(file, edits)?;
        }
        Ok(())
    }

    // Patch one file in memory, then temp-file + fsync + rename over it;
    // the original bytes go to `backups` for rollback.
    fn rewrite_one(&mut self, file: &Path, edits: &[Edit]) -> JmoveResult<()> {
        let path = self.root.join(file);
        let original = fs::read(&path)?;
        let patched = rewrite_bytes(file, &original, edits)?;
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
        for (src, dst, via_git) in self.moved.drain(..).rev() {
            // The prune step never runs before a failure, but a later move
            // can fail after git mv created target dirs that a rollback
            // through git may expect; ensure the original parent exists.
            let _ = fs::create_dir_all(self.root.join(&src).parent().unwrap());
            let back = if via_git {
                git::mv(&self.root, &dst, &src)
            } else {
                fs::rename(self.root.join(&dst), self.root.join(&src)).map_err(Into::into)
            };
            if let Err(e) = back {
                problems.push(format!("could not move back {}: {e}", rel_str(&dst)));
            }
        }
        for (file, bytes) in self.backups.drain(..).rev() {
            if let Err(e) = fs::write(self.root.join(&file), &bytes) {
                problems.push(format!("could not restore {}: {e}", rel_str(&file)));
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
