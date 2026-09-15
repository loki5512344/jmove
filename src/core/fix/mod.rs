//! Fix planning: run the language rules over the indexed files and gather
//! their candidates. Like a [`crate::core::plan::MovePlan`], a [`FixPlan`]
//! is pure data, so `--dry-run` and `--json` render it without any write,
//! and applying reuses the atomic engine behind `mv`.
//!
//! Overlap policy: candidates from different rules may touch the same
//! bytes (`import-order` rewrites the whole block a `unused-import`
//! deletion sits in). [`prune_overlaps`] resolves that per file — urgent
//! rules win, the loser is downgraded to a manual "skipped" report and
//! converges on the next run — instead of rejecting the whole plan. The
//! apply engine's `PLAN_REJECTED` guard stays as the backstop.

use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::PathBuf;

use crate::core::Edit;
use crate::core::index::Index;
use crate::parser::{FixCandidate, SourceLanguage, fixers_for};

/// Every candidate found by one `jmove fix` run, grouped per file: files
/// in sorted order, candidates of a file in source order.
#[derive(Debug, Default)]
pub struct FixPlan {
    /// Project-relative file → its candidates (non-empty groups only).
    pub files: BTreeMap<PathBuf, Vec<FixCandidate>>,
}

impl FixPlan {
    /// `true` when no rule matched anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Total candidates across all files.
    #[must_use]
    pub fn total(&self) -> usize {
        self.files.values().map(Vec::len).sum()
    }

    /// The auto-fixable candidates as per-file edit groups, ready for the
    /// apply/diff engine; files with only manual candidates drop out.
    #[must_use]
    pub fn auto_edits(&self) -> BTreeMap<PathBuf, Vec<Edit>> {
        self.files
            .iter()
            .map(|(file, candidates)| {
                let edits: Vec<Edit> = candidates
                    .iter()
                    .filter(|c| c.auto_fixable)
                    .flat_map(|c| c.edits.iter().cloned())
                    .collect();
                (file.clone(), edits)
            })
            .filter(|(_, edits)| !edits.is_empty())
            .collect()
    }
}

/// Run every rule for each indexed file's language — or only the rule
/// named by `rule` — over freshly-read contents. Files that vanished or
/// are not valid UTF-8 after indexing are skipped, mirroring the scanner.
#[must_use]
pub fn plan_fix(index: &Index, rule: Option<&str>) -> FixPlan {
    let mut plan = FixPlan::default();
    for file in index.files.sorted() {
        let Some(lang) = SourceLanguage::for_path(&file) else {
            continue;
        };
        let Ok(source) = fs::read_to_string(index.root.join(&file)) else {
            continue;
        };
        let mut found: Vec<FixCandidate> = fixers_for(lang)
            .iter()
            .filter(|fixer| rule.is_none_or(|name| fixer.rule() == name))
            .flat_map(|fixer| fixer.fixes(&file, &source, index))
            .collect();
        if found.is_empty() {
            continue;
        }
        prune_overlaps(&mut found);
        found.sort_by_key(|candidate| candidate.span.start);
        plan.files.insert(file, found);
    }
    plan
}

// Resolve cross-rule byte conflicts before the engine ever sees them:
// accept candidates by (severity, position), and downgrade any whose edits
// touch an accepted span to a manual report ("skipped") with its edits
// dropped. The apply run is deterministic and the next `fix` sees the
// re-written file, so overlapping fixes converge over consecutive runs
// instead of rejecting the whole plan. Empty spans (insertions) conflict
// only when strictly inside another span, so inserts at a replaced block's
// boundary coexist with the replacement.
fn prune_overlaps(candidates: &mut [FixCandidate]) {
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by_key(|&i| (candidates[i].severity.rank(), candidates[i].span.start));
    let mut accepted: Vec<Range<usize>> = Vec::new();
    for i in order {
        let candidate = &mut candidates[i];
        let conflict = candidate.edits.iter().any(|e| {
            accepted
                .iter()
                .any(|a| a.start < e.span.end && e.span.start < a.end)
        });
        if conflict {
            candidate.edits.clear();
            candidate.auto_fixable = false;
            candidate
                .message
                .push_str(" (skipped: overlaps a higher-priority fix, re-run after applying)");
        } else {
            accepted.extend(candidate.edits.iter().map(|e| e.span.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{plan_fix, prune_overlaps};
    use crate::core::Edit;
    use crate::core::JmoveResult;
    use crate::core::index::Index;
    use crate::parser::FixCandidate;
    use std::fs;

    fn rule_fixture(source: &str) -> JmoveResult<tempfile::TempDir> {
        let dir = tempfile::TempDir::new()?;
        let file = dir.path().join("src/main/java/p/A.java");
        fs::create_dir_all(file.parent().unwrap())?;
        fs::write(file, source)?;
        Ok(dir)
    }

    #[test]
    fn plan_groups_candidates_and_filters_by_rule() -> JmoveResult<()> {
        let dir = rule_fixture("package p;\n\nimport a.b.User;\n\nclass C {}\n")?;
        let index = Index::build(dir.path())?;
        let plan = plan_fix(&index, None);
        assert_eq!(plan.total(), 1);
        assert_eq!(plan.auto_edits().len(), 1);
        // Filtering on a rule that exists keeps the candidate...
        assert_eq!(plan_fix(&index, Some("java/unused-import")).total(), 1);
        // ...and an unknown one yields an empty plan.
        assert!(plan_fix(&index, Some("ts/unused-import")).is_empty());
        Ok(())
    }

    #[test]
    fn clean_project_yields_empty_plan() -> JmoveResult<()> {
        let dir = rule_fixture("package p;\n\nimport a.b.User;\n\nclass C { User u; }\n")?;
        let index = Index::build(dir.path())?;
        assert!(plan_fix(&index, None).is_empty());
        Ok(())
    }

    fn candidate(
        rule: &'static str,
        severity: crate::parser::Severity,
        edits: Vec<Edit>,
    ) -> FixCandidate {
        let span = edits.first().map_or(0..0, |e| e.span.clone());
        FixCandidate {
            rule,
            message: rule.to_owned(),
            severity,
            auto_fixable: !edits.is_empty(),
            span,
            edits,
            candidates: Vec::new(),
        }
    }

    fn edit(start: usize, end: usize) -> Edit {
        Edit {
            span: start..end,
            old_text: String::new(),
            new_text: String::new(),
        }
    }

    #[test]
    fn overlapping_lower_severity_candidate_is_deferred() {
        use crate::parser::Severity;
        let mut found = vec![
            candidate("order", Severity::Info, vec![edit(0, 40)]),
            candidate("unused", Severity::Warning, vec![edit(10, 20)]),
        ];
        prune_overlaps(&mut found);
        // The urgent deletion survives; the whole-block rewrite waits.
        assert!(found[1].auto_fixable);
        assert!(!found[0].auto_fixable);
        assert!(found[0].edits.is_empty());
        assert!(found[0].message.contains("skipped"));
    }

    #[test]
    fn boundary_touching_and_disjoint_edits_coexist() {
        use crate::parser::Severity;
        // Insertion exactly at the replaced block's end byte: not a
        // conflict (the engine sorts stable and applies back-to-front).
        let mut found = vec![
            candidate("order", Severity::Info, vec![edit(0, 40)]),
            candidate("missing", Severity::Error, vec![edit(40, 40)]),
        ];
        prune_overlaps(&mut found);
        assert!(found.iter().all(|c| c.auto_fixable), "no deferral expected");
    }
}
