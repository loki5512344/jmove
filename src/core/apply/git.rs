//! Git integration: move tracked files through `git mv` so the rename
//! lands in the index (staged, with history detection) instead of being a
//! plain filesystem rename. Shells out to the system `git` — no new
//! dependency, and `git` is the only sane implementation of its own index.

use std::path::Path;
use std::process::Command;

use crate::core::{JmoveError, JmoveResult, rel_str};

/// Whether `mv` may involve git in the physical rename.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GitMode {
    /// `git mv` when the source is tracked; plain rename otherwise.
    #[default]
    Auto,
    /// Never touch git (CLI `--no-git`).
    Disabled,
}

impl GitMode {
    /// CLI mapping: the `--no-git` flag switches to [`GitMode::Disabled`].
    #[must_use]
    pub const fn from_no_git(no_git: bool) -> Self {
        if no_git { Self::Disabled } else { Self::Auto }
    }
}

/// True when `rel` (root-relative) is tracked in the git index of the
/// repository containing `root`. A missing git binary, a non-repository
/// root or an untracked file all count as "not tracked".
pub(super) fn is_tracked(root: &Path, rel: &Path) -> bool {
    let rel = rel_str(rel);
    git_output(root, &["ls-files", "--", &rel]).is_some_and(|out| !out.trim().is_empty())
}

/// True when a rename of `rel` under `root` would go through `git mv`.
#[must_use]
pub fn would_use_git(root: &Path, rel: &Path, mode: GitMode) -> bool {
    mode == GitMode::Auto && is_tracked(root, rel)
}

/// `git mv <src> <dst>` with root-relative paths; stderr on failure is
/// surfaced as [`JmoveError::Git`].
pub(super) fn mv(root: &Path, src: &Path, dst: &Path) -> JmoveResult<()> {
    let (src, dst) = (rel_str(src), rel_str(dst));
    git_run(root, &["mv", "--", &src, &dst])
}

// Run git inside `root`; None when git is absent or exits non-zero.
fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let ok = git_command(root, args).output().ok()?;
    ok.status
        .success()
        .then(|| String::from_utf8_lossy(&ok.stdout).into_owned())
}

fn git_run(root: &Path, args: &[&str]) -> JmoveResult<()> {
    let out = git_command(root, args).output()?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    Err(JmoveError::Git(format!(
        "`git {}` failed: {err}",
        args.first().copied().unwrap_or("")
    )))
}

fn git_command(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args(args);
    cmd
}

#[cfg(test)]
mod tests {
    use super::{GitMode, is_tracked, mv, would_use_git};
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    // A real throwaway repository: init, one committed file, no user
    // identity needed beyond the inline -c overrides.
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().expect("tempdir");
        git(dir.path(), &["init", "-q", "-b", "main", "."]);
        fs::write(dir.path().join("a.txt"), "one\n").expect("write");
        git(dir.path(), &["add", "a.txt"]);
        git(
            dir.path(),
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "init",
            ],
        );
        dir
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .expect("git");
        assert!(status.success(), "`git {args:?}` failed");
    }

    fn stdout(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "`git {args:?}` failed");
        String::from_utf8(out.stdout).expect("utf-8")
    }

    #[test]
    fn tracks_only_indexed_files() {
        let dir = repo();
        let root = dir.path();
        assert!(is_tracked(root, Path::new("a.txt")));
        assert!(!is_tracked(root, Path::new("untracked.txt")));
        // Not a repository at all.
        let plain = tempfile::TempDir::new().expect("tempdir");
        assert!(!is_tracked(plain.path(), Path::new("a.txt")));
    }

    #[test]
    fn would_use_git_respects_mode_and_tracking() {
        let dir = repo();
        let root = dir.path();
        assert!(would_use_git(root, Path::new("a.txt"), GitMode::Auto));
        assert!(!would_use_git(root, Path::new("a.txt"), GitMode::Disabled));
        assert!(!would_use_git(root, Path::new("nope.txt"), GitMode::Auto));
    }

    #[test]
    fn from_no_git_maps_the_cli_flag() {
        assert_eq!(GitMode::from_no_git(false), GitMode::Auto);
        assert_eq!(GitMode::from_no_git(true), GitMode::Disabled);
        assert_eq!(GitMode::default(), GitMode::Auto);
    }

    #[test]
    fn mv_renames_on_disk_and_stages_the_rename() {
        let dir = repo();
        let root = dir.path();
        fs::create_dir_all(root.join("sub")).expect("mkdir");
        mv(root, Path::new("a.txt"), Path::new("sub/a.txt")).expect("git mv");
        assert!(!root.join("a.txt").exists());
        assert!(root.join("sub/a.txt").is_file());
        let staged = stdout(root, &["diff", "--cached", "-M", "--name-status"]);
        assert_eq!(staged, "R100\ta.txt\tsub/a.txt\n");
    }
}
