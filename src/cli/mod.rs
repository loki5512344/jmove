//! Command line interface: argument parsing, dispatch and exit codes.
//!
//! Exit codes (mirrored in `docs/SKILL.md`):
//! `0` success · `1` operation error · `2` broken imports found.

pub mod fix;
pub mod json;
pub mod output;

use std::convert::identity;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::core::apply;
use crate::core::index::Index;
use crate::core::plan::{self, MovePlan};
use crate::core::{self, JmoveError, JmoveResult};

use json::{Envelope, ErrorData, MvData, MvDryRunData};

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
    /// Index only files under this root-relative directory: the monorepo
    /// disambiguator for duplicate Java packages (`guava` vs `android/guava`).
    #[arg(long, global = true)]
    pub source_root: Option<PathBuf>,
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
        /// Plain filesystem rename even for git-tracked files.
        #[arg(long)]
        no_git: bool,
    },
    /// Report broken imports in the project.
    Check {
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Auto-fix small import problems (same engine as `mv`).
    Fix {
        /// Run only this rule id (see docs/SKILL.md), e.g. java/unused-import.
        #[arg(long)]
        rule: Option<String>,
        /// Preview changes without touching the disk.
        #[arg(long)]
        dry_run: bool,
        /// Machine-readable JSON output (for AI agents).
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
            no_git,
        } => mv(
            &args.root,
            args.source_root.as_deref(),
            &source,
            &target,
            dry_run,
            json,
            no_git,
        ),
        Command::Check { json } => check(&args.root, args.source_root.as_deref(), json),
        Command::Fix {
            rule,
            dry_run,
            json,
        } => fix::fix(
            &args.root,
            args.source_root.as_deref(),
            rule.as_deref(),
            dry_run,
            json,
        ),
    };
    // Handlers report their own failures; both arms carry an exit code.
    Ok(outcome.unwrap_or_else(identity))
}

/// `mv` handler: normalize paths, validate, index, plan, then dry-run or apply.
fn mv(
    root: &Path,
    source_root: Option<&Path>,
    source: &Path,
    target: &Path,
    dry_run: bool,
    json: bool,
    no_git: bool,
) -> Flow<i32> {
    let git = apply::GitMode::from_no_git(no_git);
    let root = flow(json, "mv", root.canonicalize().map_err(JmoveError::from))?;
    let source_root = flow(json, "mv", normalize_scope(&root, source_root))?;
    let source = flow(json, "mv", core::rel_from_root(&root, source))?;
    let target = flow(json, "mv", core::rel_from_root(&root, target))?;
    if let Some(rejected) = output::mv_reject(&root, &source, &target) {
        return Err(fail(json, "mv", rejected));
    }

    let scope = source_root.as_deref();
    let index = flow(json, "mv", Index::build_scoped(&root, scope))?;
    let plan = flow(json, "mv", plan::plan_move(&index, &source, &target))?;
    if dry_run {
        return mv_dry_run(&root, json, &plan, git);
    }
    // Line numbers use spans against the *original* contents, so the JSON
    // payload is assembled before the rewrites hit the disk.
    let changed = if json {
        flow(json, "mv", output::changed_files(&root, &plan))?
    } else {
        Vec::new()
    };
    let applied = flow(json, "mv", apply::apply(&root, &plan, git))?;

    if json {
        json::print(&Envelope::ok(
            "mv",
            MvData::new(&plan, changed, applied.via_git),
        ));
    } else {
        println!("{}", output::mv_summary(&plan, applied.via_git));
    }
    Ok(exit::OK)
}

/// Dry-run branch: unified diff for humans, structured preview for agents.
fn mv_dry_run(root: &Path, json: bool, plan: &MovePlan, git: apply::GitMode) -> Flow<i32> {
    let diff = flow(json, "mv", apply::render_diff(root, plan))?;
    let via_git = apply::would_use_git(root, &plan.source, git);
    if json {
        json::print(&Envelope::dry_run(
            "mv",
            MvDryRunData::new(plan, diff, via_git),
        ));
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
fn check(root: &Path, source_root: Option<&Path>, json: bool) -> Flow<i32> {
    let root = flow(json, "check", root.canonicalize().map_err(JmoveError::from))?;
    let source_root = flow(json, "check", normalize_scope(&root, source_root))?;
    let scope = source_root.as_deref();
    let index = flow(json, "check", Index::build_scoped(&root, scope))?;
    let broken = flow(json, "check", output::broken_imports(&root, &index))?;
    let mismatches = flow(json, "check", output::name_mismatches(&root, &index))?;
    let code = if broken.is_empty() && mismatches.is_empty() {
        exit::OK
    } else {
        exit::BROKEN
    };

    if json {
        let total = broken.len();
        let data = output::CheckData {
            broken_imports: broken,
            total,
            name_mismatches: mismatches,
        };
        json::print(&Envelope::ok("check", data));
    } else {
        output::report_check(&broken, &mismatches);
    }
    Ok(code)
}

/// Validate the global `--source-root`: project-relative, existing dir.
fn normalize_scope(root: &Path, scope: Option<&Path>) -> JmoveResult<Option<PathBuf>> {
    let Some(scope) = scope else {
        return Ok(None);
    };
    let rel = core::rel_from_root(root, scope)?;
    if !root.join(&rel).is_dir() {
        return Err(JmoveError::InvalidArgument(format!(
            "--source-root '{}' is not a directory",
            core::rel_str(&rel)
        )));
    }
    Ok(Some(rel))
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
