//! Unified-diff rendering of edit plans, used by `--dry-run` of every
//! command: [`render_edits_diff`] is the generic engine over per-file edit
//! groups, [`render_diff`] adds the `mv` rename line.

use similar::TextDiff;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::core::Edit;
use crate::core::apply::fsops::{group_by_file, rewrite_bytes};
use crate::core::plan::MovePlan;
use crate::core::{JmoveResult, rel_str};

/// Render the plan as a unified diff per rewritten file plus a final
/// `move <source> -> <target>` line, for dry-run. A plan without rewrites
/// renders the empty string.
pub fn render_diff(root: &Path, plan: &MovePlan) -> JmoveResult<String> {
    let mut out = render_edits_diff(root, &group_by_file(plan))?;
    if !out.is_empty() {
        let (src, dst) = (rel_str(&plan.source), rel_str(&plan.target));
        out.push_str(&format!("move {src} -> {dst}\n"));
    }
    Ok(out)
}

/// Render pre-grouped edits (the `fix` dry-run shape) as one unified diff
/// per changed file; files with empty edit groups render nothing.
pub fn render_edits_diff(
    root: &Path,
    by_file: &BTreeMap<PathBuf, Vec<Edit>>,
) -> JmoveResult<String> {
    let mut out = String::new();
    for (file, edits) in by_file {
        let original = std::fs::read(root.join(file))?;
        let patched = rewrite_bytes(file, &original, edits)?;
        let name = rel_str(file);
        let old = String::from_utf8_lossy(&original);
        let new = String::from_utf8_lossy(&patched);
        let text = TextDiff::from_lines(&old, &new);
        out.push_str(&text.unified_diff().header(&name, &name).to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::render_diff;
    use crate::core::JmoveResult;
    use crate::core::plan::{MovePlan, Rewrite};
    use std::path::Path;

    const OLD: &str = "import {\n  fmt,\n} from '../lib/fmt';\n";

    fn plan() -> MovePlan {
        MovePlan {
            source: "lib/fmt.ts".into(),
            target: "deep/fmt.ts".into(),
            rewrites: vec![Rewrite {
                file: "src/app.ts".into(),
                span: 24..34,
                old_text: "../lib/fmt".into(),
                new_text: "../deep/fmt".into(),
            }],
        }
    }

    #[test]
    fn render_diff_smoke() -> JmoveResult<()> {
        let dir = tempfile::TempDir::new()?;
        std::fs::create_dir_all(dir.path().join("src"))?;
        std::fs::write(dir.path().join("src/app.ts"), OLD)?;
        let diff = render_diff(dir.path(), &plan())?;
        assert!(diff.contains("--- src/app.ts") && diff.contains("+++ src/app.ts"));
        assert!(diff.contains("@@"));
        assert!(diff.contains("-} from '../lib/fmt';") && diff.contains("+} from '../deep/fmt';"));
        assert!(diff.ends_with("move lib/fmt.ts -> deep/fmt.ts\n"));
        let mut empty = plan();
        empty.rewrites.clear();
        assert_eq!(render_diff(dir.path(), &empty)?, "");
        Ok(())
    }

    #[test]
    fn render_diff_reads_from_root() -> JmoveResult<()> {
        // Relative-root sanity: same content, root passed as `.` style path.
        let dir = tempfile::TempDir::new()?;
        std::fs::create_dir_all(dir.path().join("src"))?;
        std::fs::write(dir.path().join("src/app.ts"), OLD)?;
        let abs: &Path = dir.path();
        let diff = render_diff(abs, &plan())?;
        assert!(diff.contains("../deep/fmt"));
        Ok(())
    }
}
