//! Low-level byte and filesystem helpers shared by apply and diff.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::Edit;
use crate::core::plan::MovePlan;
use crate::core::{JmoveError, JmoveResult, rel_str};

// Group plan rewrites as generic per-file edits; the BTreeMap keeps the
// order deterministic.
pub(super) fn group_by_file(plan: &MovePlan) -> BTreeMap<PathBuf, Vec<Edit>> {
    let mut m: BTreeMap<PathBuf, Vec<Edit>> = BTreeMap::new();
    for r in &plan.rewrites {
        m.entry(r.file.clone()).or_default().push(r.into());
    }
    m
}

// `<file>.jmove-tmp` next to `path` (same dir => same filesystem).
pub(super) fn sibling_temp(path: &Path) -> PathBuf {
    let mut temp = path.as_os_str().to_os_string();
    temp.push(".jmove-tmp");
    PathBuf::from(temp)
}

// Byte `i` is a UTF-8 char boundary iff it sits at/after the end or is
// not a continuation byte (`10xx_xxxx`).
fn is_char_boundary(bytes: &[u8], i: usize) -> bool {
    bytes.get(i).is_none_or(|b| b & 0xC0 != 0x80)
}

// Apply edits in reverse span order so earlier offsets stay valid; a
// span/content mismatch means the plan is stale. Equal starts (insertions
// at one offset) keep their input order in the output because the
// stable-ascending sort is applied back-to-front. Overlapping or
// malformed spans are generator bugs and rejected before any byte of
// `out` changes. Non-empty needles can only match valid UTF-8 on char
// boundaries; the explicit boundary check pins insertions the same way,
// so patching UTF-8 stays UTF-8.
pub(super) fn rewrite_bytes(file: &Path, original: &[u8], edits: &[Edit]) -> JmoveResult<Vec<u8>> {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|e| (e.span.start, e.span.end));
    if sorted.iter().any(|e| e.span.start > e.span.end)
        || sorted.windows(2).any(|w| w[1].span.start < w[0].span.end)
    {
        return Err(JmoveError::PlanRejected(format!(
            "overlapping or malformed edits in '{}': {:?}",
            rel_str(file),
            edits.iter().map(|e| e.span.clone()).collect::<Vec<_>>()
        )));
    }
    let mut out = original.to_vec();
    for e in sorted.iter().rev() {
        if !is_char_boundary(&out, e.span.start)
            || e.span.end > out.len()
            || &out[e.span.clone()] != e.old_text.as_bytes()
        {
            return Err(JmoveError::StaleIndex(format!(
                "'{}' changed since indexing (expected {:?} at {:?})",
                rel_str(file),
                e.old_text,
                e.span
            )));
        }
        out.splice(e.span.clone(), e.new_text.as_bytes().iter().copied());
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

#[cfg(test)]
mod tests {
    use super::rewrite_bytes;
    use crate::core::{Edit, JmoveError};
    use std::path::Path;

    fn file() -> &'static Path {
        Path::new("f.txt")
    }

    fn edit(start: usize, end: usize, old: &str, new: &str) -> Edit {
        Edit {
            span: start..end,
            old_text: old.into(),
            new_text: new.into(),
        }
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).expect("valid utf-8")
    }

    #[test]
    fn replaces_inserts_and_deletes_in_one_pass() {
        // "one\ntwo\nthree\n": insert a line at 0, replace `two`, delete
        // the whole `three` line via its span absorbing the trailing `\n`.
        let src = b"one\ntwo\nthree\n";
        let edits = [
            edit(4, 7, "two", "TWO"),
            edit(8, 14, "three\n", ""),
            edit(0, 0, "", "zero\n"),
        ];
        let out = rewrite_bytes(file(), src, &edits).unwrap();
        assert_eq!(text(&out), "zero\none\nTWO\n");
    }

    #[test]
    fn insertions_at_one_offset_keep_input_order() {
        let src = b"head\ntail\n";
        let edits = [edit(5, 5, "", "b\n"), edit(5, 5, "", "a\n")];
        let out = rewrite_bytes(file(), src, &edits).unwrap();
        assert_eq!(text(&out), "head\nb\na\ntail\n");
    }

    #[test]
    fn overlapping_or_malformed_spans_are_rejected() {
        let src = b"abcdefgh";
        let overlap = [edit(2, 6, "cdef", "X"), edit(4, 8, "efgh", "Y")];
        let err = rewrite_bytes(file(), src, &overlap).unwrap_err();
        assert!(matches!(err, JmoveError::PlanRejected(_)), "{err}");
        let malformed = [edit(6, 2, "", "X")];
        let err = rewrite_bytes(file(), src, &malformed).unwrap_err();
        assert!(matches!(err, JmoveError::PlanRejected(_)), "{err}");
    }

    #[test]
    fn stale_or_off_boundary_edits_are_rejected() {
        // Wrong expectation at the span => the file moved under the plan.
        let err = rewrite_bytes(file(), b"abcd", &[edit(0, 4, "wrong", "X")]).unwrap_err();
        assert!(matches!(err, JmoveError::StaleIndex(_)), "{err}");
        // Insertion inside the 2-byte `ä` is not a char boundary.
        let err = rewrite_bytes(file(), "ä\n".as_bytes(), &[edit(1, 1, "", "x")]).unwrap_err();
        assert!(matches!(err, JmoveError::StaleIndex(_)), "{err}");
    }
}
