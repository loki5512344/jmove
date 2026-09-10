//! Command line interface: argument parsing, dispatch and exit codes.
//!
//! Exit codes (mirrored in `docs/SKILL.md`):
//! `0` success · `1` operation error · `2` broken imports found.

pub mod json;
pub mod output;

use std::convert::identity;
use std::path::{Component, Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::core::apply;
use crate::core::index::Index;
use crate::core::plan::{self, MovePlan};
use crate::core::{self, JmoveError, JmoveResult};

use json::{CheckData, Envelope, ErrorData, MvData, MvDryRunData};

/// jmove — move source files, keep every import intact.
#[derive(Debug, Parser)]
#[command(name = "jmove", version, about, long_about = None)]
pub struct Args {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
    /// Project root (defaults to the current directory).
    #[arg(long, global = true, default_value = ".")]
    pub root: PathBuf,
}

/// Available subcommands (MVP: `mv`, `check`).
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Move a file and rewrite all imports referencing it.
    Mv {
        /// File being moved (project-relative or inside the root).
        source: PathBuf,
        /// Destination path.
        target: PathBuf,
        /// Preview changes without touching the disk.
        #[arg(long)]
        dry_run: bool,
        /// Machine-readable JSON output (for AI agents).
        #[arg(long)]
        json: bool,
    },
    /// Report broken imports in the project.
    Check {
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}

/// Process exit codes documented for humans and agents alike.
pub mod exit {
    /// Operation completed successfully.
    pub const OK: i32 = 0;
    /// Invalid usage, rejected plan or IO failure.
    pub const ERROR: i32 = 1;
    /// `check` found at least one broken import.
    pub const BROKEN: i32 = 2;
}

/// Handler flow: `Ok(value)` continues, `Err(code)` means the failure was
/// already reported to the user and the process must exit with `code`.
type Flow<T> = Result<T, i32>;

/// Parse arguments and run the selected command.
/// Returns the process exit code; `Err` is reserved for unexpected failures.
pub fn run() -> anyhow::Result<i32> {
    let args = Args::parse();
    let outcome = match args.command {
        Command::Mv {
            source,
            target,
            dry_run,
            json,
        } => mv(&args.root, &source, &target, dry_run, json),
        Command::Check { json } => check(&args.root, json),
    };
    // Handlers report their own failures; both arms carry an exit code.
    Ok(outcome.unwrap_or_else(identity))
}

/// `mv` handler: normalize paths, validate, index, plan, then dry-run or apply.
fn mv(root: &Path, source: &Path, target: &Path, dry_run: bool, json: bool) -> Flow<i32> {
    let root = flow(json, "mv", root.canonicalize().map_err(JmoveError::from))?;
    let source = flow(json, "mv", rel_from_root(&root, source))?;
    let target = flow(json, "mv", rel_from_root(&root, target))?;
    if let Some(rejected) = mv_reject(&root, &source, &target) {
        return Err(fail(json, "mv", rejected));
    }

    let index = flow(json, "mv", Index::build(&root))?;
    let plan = flow(json, "mv", plan::plan_move(&index, &source, &target))?;
    if dry_run {
        return mv_dry_run(&root, json, &plan);
    }
    // Line numbers use spans against the *original* contents, so the JSON
    // payload is assembled before the rewrites hit the disk.
    let changed = if json {
        flow(json, "mv", output::changed_files(&root, &plan))?
    } else {
        Vec::new()
    };
    flow(json, "mv", apply::apply(&root, &plan))?;

    if json {
        json::print(&Envelope::ok("mv", MvData::new(&plan, changed)));
    } else {
        println!("{}", output::mv_summary(&plan));
    }
    Ok(exit::OK)
}

/// Pre-flight `mv` validation. A file that exists on disk but is absent
/// from the import index stays moveable: its plan simply has no rewrites.
fn mv_reject(root: &Path, source: &Path, target: &Path) -> Option<ErrorData> {
    let bad = |code: &str, message: String, hint: &str| {
        Some(ErrorData::new(code, message, Some(hint.into())))
    };
    if source == target {
        let msg = "source and target are the same path".into();
        return bad("INVALID_ARGUMENT", msg, "pick a different destination");
    }
    if !root.join(source).is_file() {
        let msg = format!("source file '{}' does not exist", source.display());
        return bad(
            "SOURCE_NOT_FOUND",
            msg,
            "check the path or run `jmove check`",
        );
    }
    if root.join(target).exists() {
        let msg = format!("target path '{}' already exists", target.display());
        return bad(
            "TARGET_EXISTS",
            msg,
            "remove or rename the existing target first",
        );
    }
    // `target` names a file, so `parent()` always yields the directory part.
    let parent = root.join(target.parent().unwrap_or(Path::new("")));
    if parent.exists() && !parent.is_dir() {
        let msg = format!("target parent of '{}' is not a directory", target.display());
        return bad(
            "INVALID_ARGUMENT",
            msg,
            "pick a destination inside a directory",
        );
    }
    None
}

/// Dry-run branch: unified diff for humans, structured preview for agents.
fn mv_dry_run(root: &Path, json: bool, plan: &MovePlan) -> Flow<i32> {
    let diff = flow(json, "mv", apply::render_diff(root, plan))?;
    if json {
        json::print(&Envelope::dry_run("mv", MvDryRunData::new(plan, diff)));
    } else {
        print!("{diff}");
    }
    Ok(exit::OK)
}

/// `check` handler: report relative imports that resolve to nothing.
///
/// Exit code is `2` when at least one broken import was found, in both the
/// human and the `--json` mode (the JSON `status` stays `"ok"` — the
/// command itself succeeded; agents read `total` or the exit code).
fn check(root: &Path, json: bool) -> Flow<i32> {
    let root = flow(json, "check", root.canonicalize().map_err(JmoveError::from))?;
    let index = flow(json, "check", Index::build(&root))?;
    let broken = flow(json, "check", output::broken_imports(&root, &index))?;
    let code = if broken.is_empty() {
        exit::OK
    } else {
        exit::BROKEN
    };

    if json {
        let total = broken.len();
        let data = CheckData {
            broken_imports: broken,
            total,
        };
        json::print(&Envelope::ok("check", data));
    } else {
        output::report_check(&broken);
    }
    Ok(code)
}

/// Unwrap a core result, routing failures through the CLI error channel.
fn flow<T>(json: bool, operation: &'static str, result: JmoveResult<T>) -> Flow<T> {
    result.map_err(|err| fail(json, operation, ErrorData::from_core(&err)))
}

/// Report `err` as a JSON envelope or stderr lines; return the exit code.
fn fail(json: bool, operation: &'static str, err: ErrorData) -> i32 {
    if json {
        json::print(&Envelope::error(operation, err));
    } else {
        output::print_error(&err.message, err.hint.as_deref());
    }
    exit::ERROR
}

/// Convert a user path to a normalized project-relative path. Relative
/// paths are taken against `root`; absolute ones must live underneath it.
fn rel_from_root(root: &Path, path: &Path) -> JmoveResult<PathBuf> {
    let joined = if path.is_absolute() {
        path.into()
    } else {
        root.join(path)
    };
    let outside = || {
        JmoveError::InvalidArgument(format!(
            "path '{}' is outside the project root",
            path.display()
        ))
    };
    let abs = collapse(&joined);
    let rel = abs.strip_prefix(root).map_err(|_| outside())?;
    core::normalize_rel_path(rel).ok_or_else(|| {
        JmoveError::InvalidArgument(format!("invalid project path '{}'", path.display()))
    })
}

/// Lexically normalize a path: drop `.` segments, apply `..` where possible.
fn collapse(path: &Path) -> PathBuf {
    let mut stack: Vec<Component<'_>> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.last() != Some(&Component::ParentDir) {
                    stack.pop();
                }
            }
            other => stack.push(other),
        }
    }
    stack.into_iter().collect()
}
