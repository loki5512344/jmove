//! Low-level byte and filesystem helpers shared by apply and diff.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::plan::{MovePlan, Rewrite};
use crate::core::{JmoveError, JmoveResult};

// Group rewrites by file; the BTreeMap keeps the order deterministic.
pub(super) fn group_by_file(plan: &MovePlan) -> BTreeMap<PathBuf, Vec<&Rewrite>> {
    let mut m: BTreeMap<PathBuf, Vec<&Rewrite>> = BTreeMap::new();
    for r in &plan.rewrites {
        m.entry(r.file.clone()).or_default().push(r);
    }
    m
}

// `<file>.jmove-tmp` next to `path` (same dir => same filesystem).
pub(super) fn sibling_temp(path: &Path) -> PathBuf {
    let mut temp = path.as_os_str().to_os_string();
    temp.push(".jmove-tmp");
    PathBuf::from(temp)
}

// Apply byte-span replacements in reverse offset order so earlier spans
// stay valid; a span/content mismatch means the plan is stale. Valid
// UTF-8 needles can only match on char boundaries, so a successful
// rewrite of valid UTF-8 stays valid UTF-8.
pub(super) fn rewrite_bytes(original: &[u8], rewrites: &[&Rewrite]) -> JmoveResult<Vec<u8>> {
    let mut out = original.to_vec();
    let mut sorted = rewrites.to_vec();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.span.start));
    for r in sorted {
        if r.span.end > out.len() || &out[r.span.clone()] != r.old_text.as_bytes() {
            return Err(JmoveError::StaleIndex(format!(
                "'{}' changed since indexing (expected {:?} at {:?})",
                r.file.display(),
                r.old_text,
                r.span
            )));
        }
        out.splice(r.span.clone(), r.new_text.as_bytes().iter().copied());
    }
    Ok(out)
}

// Write `bytes` to `temp` durably: create, write, flush, fsync.
pub(super) fn write_durable(temp: &Path, bytes: &[u8]) -> JmoveResult<()> {
    let mut file = fs::File::create(temp)?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(file.sync_all()?)
}

// Create the missing parent dirs of `dst`; returns the ones actually
// created (innermost last) so rollback can remove them in reverse.
pub(super) fn create_missing_dirs(dst: &Path) -> JmoveResult<Vec<PathBuf>> {
    let mut created = Vec::new();
    let mut current = dst.parent().unwrap_or(Path::new("")).to_path_buf();
    while !current.exists() {
        created.push(current.clone());
        let Some(up) = current.parent() else { break };
        current = up.to_path_buf();
    }
    for dir in created.iter().rev() {
        fs::create_dir(dir)?;
    }
    Ok(created)
}
