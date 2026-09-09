//! Command line interface: argument parsing, dispatch and exit codes.
//!
//! Exit codes (mirrored in `docs/SKILL.md`):
//! `0` success · `1` operation error · `2` broken imports found.

pub mod json;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
        /// Allow overwriting an existing target file.
        #[arg(long)]
        force: bool,
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

/// Parse arguments and run the selected command.
/// Returns the process exit code; `Err` is reserved for unexpected failures.
pub fn run() -> anyhow::Result<i32> {
    let args = Args::parse();
    match args.command {
        Command::Mv {
            source,
            target,
            dry_run,
            json,
            force,
        } => mv(&args.root, &source, &target, dry_run, json, force),
        Command::Check { json } => check(&args.root, json),
    }
}

/// `mv` handler: build index, plan, then dry-run-print or apply.
fn mv(
    root: &Path,
    source: &Path,
    target: &Path,
    dry_run: bool,
    json: bool,
    force: bool,
) -> anyhow::Result<i32> {
    let _ = (root, source, target, dry_run, json, force);
    todo!("cli agent: wire mv to core::index/plan/apply")
}

/// `check` handler: report imports that resolve to nothing.
fn check(root: &Path, json: bool) -> anyhow::Result<i32> {
    let _ = (root, json);
    todo!("cli agent: wire check to core::index")
}

use std::path::Path;
