//! `jmove` — a project-aware file mover and import fixer for
//! TypeScript/JavaScript and Java.
//!
//! Moving a file inside a project invalidates every relative import that
//! points at it. `jmove` indexes the project's import graph, computes the
//! minimal set of specifier rewrites, and applies everything atomically
//! (with rollback), optionally in dry-run mode. The same engine drives
//! `fix`, which auto-repairs small import problems via deterministic rules.
//!
//! Module map:
//! - [`cli`] — argument parsing, command dispatch, user-facing output.
//! - [`core`] — indexing, dependency graph, move/fix planning, atomic apply.
//! - [`parser`] — language frontends (import extraction, path resolution, fix rules).
//! - [`cache`] — on-disk index cache (Phase 2, intentionally empty for now).

pub mod cache;
pub mod cli;
pub mod core;
pub mod parser;
