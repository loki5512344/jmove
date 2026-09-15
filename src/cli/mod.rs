//! Command line interface: argument parsing, dispatch and exit codes.
//!
//! Exit codes (mirrored in `docs/SKILL.md`):
//! `0` success · `1` operation error · `2` broken imports found.

pub mod fix;
pub mod json;
pub mod output;
pub mod report;

use std::convert::identity;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::core::apply;
use crate::core::index::Index;
use crate::core::plan::{self, MovePlan};
use crate::core::refs;
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
        /// Write findings to a report file: `.sarif` or `.xml` (checkstyle).
        #[arg(long)]
        report: Option<PathBuf>,
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
        /// Write the candidate list to a report file: `.sarif` or `.xml`.
        #[arg(long)]
        report: Option<PathBuf>,
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
            apply::GitMode::from_no_git(no_git),
        ),
        Command::Check { json, report } => report::check(
            &args.root,
            args.source_root.as_deref(),
            json,
            report.as_deref(),
        ),
        Command::Fix {
            rule,
            dry_run,
            json,
            report,
        } => fix::fix(
            &args.root,
            args.source_root.as_deref(),
            rule.as_deref(),
            dry_run,
            json,
            report.as_deref(),
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
    git: apply::GitMode,
) -> Flow<i32> {
    let root = flow(json, "mv", root.canonicalize().map_err(JmoveError::from))?;
    let source_root = flow(json, "mv", Index::normalize_scope(&root, source_root))?;
    let source = flow(json, "mv", core::rel_from_root(&root, source))?;
    let target = flow(json, "mv", core::rel_from_root(&root, target))?;
    if let Some(rejected) = output::mv_reject(&root, &source, &target) {
        return Err(fail(json, "mv", rejected));
    }

    let scope = source_root.as_deref();
    let index = flow(json, "mv", Index::build_scoped(&root, scope))?;
    let plan = flow(json, "mv", plan::plan_move(&index, &source, &target))?;
    let hidden = refs::scan(&root, &index, &plan);
    if dry_run {
        return mv_dry_run(&root, json, &plan, git, hidden);
    }
    // Line numbers use spans against the *original* contents.
    let changed = if json {
        flow(json, "mv", output::changed_files(&root, &plan))?
    } else {
        Vec::new()
    };
    let applied = flow(json, "mv", apply::apply(&root, &plan, git))?;

    if json {
        let data = MvData::new(&plan, changed, applied.via_git, hidden);
        json::print(&Envelope::ok("mv", data));
    } else {
        println!("{}", output::mv_summary(&plan, applied.via_git));
        output::report_refs(&hidden);
    }
    Ok(exit::OK)
}

/// Dry-run branch: unified diff for humans, structured preview for agents.
fn mv_dry_run(
    root: &Path,
    json: bool,
    plan: &MovePlan,
    git: apply::GitMode,
    hidden: Vec<refs::NonImportRef>,
) -> Flow<i32> {
    let diff = flow(json, "mv", apply::render_diff(root, plan))?;
    let via_git = apply::would_use_git(root, &plan.source, git);
    if json {
        let data = MvDryRunData::new(plan, diff, via_git, hidden);
        json::print(&Envelope::dry_run("mv", data));
    } else {
        print!("{diff}");
        output::report_refs(&hidden);
    }
    Ok(exit::OK)
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
