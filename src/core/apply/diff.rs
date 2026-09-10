//! Unified-diff rendering of a plan, used by `mv --dry-run`.

use similar::TextDiff;

use crate::core::JmoveResult;
use crate::core::apply::fsops::{group_by_file, rewrite_bytes};
use crate::core::plan::MovePlan;
use std::path::Path;

/// Render the plan as a unified diff per rewritten file plus a final
/// `move <source> -> <target>` line, for dry-run. A plan without rewrites
/// renders the empty string.
pub fn render_diff(root: &Path, plan: &MovePlan) -> JmoveResult<String> {
    let mut out = String::new();
    for (file, rewrites) in group_by_file(plan) {
        let original = std::fs::read(root.join(&file))?;
        let patched = rewrite_bytes(&original, &rewrites)?;
        let name = file.display().to_string();
        let old = String::from_utf8_lossy(&original);
        let new = String::from_utf8_lossy(&patched);
        let text = TextDiff::from_lines(&old, &new);
        out.push_str(&text.unified_diff().header(&name, &name).to_string());
    }
    if !out.is_empty() {
        let (src, dst) = (plan.source.display(), plan.target.display());
        out.push_str(&format!("move {src} -> {dst}\n"));
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
