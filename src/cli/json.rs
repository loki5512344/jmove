//! JSON output shapes shared by all `--json` commands.
//!
//! Every response is an [`Envelope`] whose `status` is one of
//! `"ok" | "dry_run" | "error"`, plus a machine-readable error `code`
//! and a human `hint` on failure (see `docs/SKILL.md`).

use serde::Serialize;

/// Top-level envelope for every `--json` response.
#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    /// One of `"ok"`, `"dry_run"`, `"error"`.
    pub status: &'static str,
    /// The operation that produced this response, e.g. `"mv"`, `"check"`.
    pub operation: &'static str,
    /// Command-specific payload.
    #[serde(flatten)]
    pub data: T,
}

/// Error payload: stable `code`, human `message`, actionable `hint`.
#[derive(Debug, Serialize)]
pub struct ErrorData {
    /// Machine-readable code, e.g. `TARGET_EXISTS`, `SOURCE_NOT_FOUND`.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
    /// What the caller should do next (never null in output; omit if none).
    pub hint: Option<String>,
}

/// Serialize `value` as pretty JSON to stdout.
pub fn print<T: Serialize>(value: &T) {
    let _ = value;
    todo!("cli agent: println!(serde_json::to_string_pretty)")
}
