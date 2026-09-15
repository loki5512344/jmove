//! Direct engine tests for `apply` and `apply_edits` via the public lib
//! API: multi-move rollback and dir pruning are impossible to force
//! through the CLI (pre-flight validation catches them first).

use jmove::core::JmoveResult;
use jmove::core::apply::{GitMode, apply};
use jmove::core::plan::{FileMove, MovePlan, Rewrite};
use std::fs;
use std::path::{Path, PathBuf};

const OLD: &str = "import {\n  fmt,\n} from '../lib/fmt';\n";
const NEW: &str = "import {\n  fmt,\n} from '../deep/fmt';\n";
// Byte span of `../lib/fmt` (between the quotes) inside OLD.
const SPAN: std::ops::Range<usize> = 24..34;

// Plan moving lib/fmt.ts -> deep/fmt.ts, rewriting src/app.ts.
fn plan() -> MovePlan {
    let rewrite = Rewrite {
        file: "src/app.ts".into(),
        span: SPAN,
        old_text: "../lib/fmt".into(),
        new_text: "../deep/fmt".into(),
    };
    MovePlan {
        source: "lib/fmt.ts".into(),
        target: "deep/fmt.ts".into(),
        moves: vec![FileMove {
            source: "lib/fmt.ts".into(),
            target: "deep/fmt.ts".into(),
        }],
        rewrites: vec![rewrite],
        left_behind: Vec::new(),
        prune_dirs: Vec::new(),
    }
}

fn mk(dir: &Path, rel: &str, body: &str) -> JmoveResult<()> {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, body)?;
    Ok(())
}

#[test]
fn apply_rewrites_spans_creates_dirs_and_moves_last() -> JmoveResult<()> {
    assert_eq!(&OLD[SPAN], "../lib/fmt"); // sanity: the span is real
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    mk(root, "src/app.ts", OLD)?;
    mk(root, "lib/fmt.ts", "export const fmt = 1;\n")?;
    let applied = apply(root, &plan(), GitMode::Disabled)?;
    assert_eq!(
        (applied.files_rewritten, &applied.new_path),
        (1, &PathBuf::from("deep/fmt.ts"))
    );
    assert!(!root.join("lib/fmt.ts").exists());
    assert_eq!(
        fs::read_to_string(root.join("deep/fmt.ts"))?,
        "export const fmt = 1;\n"
    );
    // Only the specifier bytes changed; the layout is kept byte-exact.
    assert_eq!(fs::read_to_string(root.join("src/app.ts"))?, NEW);
    assert!(!root.join("src/app.ts.jmove-tmp").exists());
    Ok(())
}

#[test]
fn apply_rolls_back_when_the_move_fails() -> JmoveResult<()> {
    // Missing source: the last rename fails after the rewrite landed.
    let dir = tempfile::TempDir::new()?;
    mk(dir.path(), "src/app.ts", OLD)?;
    let err = apply(dir.path(), &plan(), GitMode::Disabled).expect_err("missing source");
    assert!(matches!(err, jmove::core::JmoveError::Io(_)), "{err}");
    // Importer restored to its exact original bytes; created dirs gone.
    assert_eq!(fs::read_to_string(dir.path().join("src/app.ts"))?, OLD);
    assert!(!dir.path().join("deep").exists());
    Ok(())
}

#[test]
fn apply_rejects_a_stale_plan_without_writing() -> JmoveResult<()> {
    // SPAN was computed on OLD's layout; a single-line importer has
    // different bytes there, so the run fails before any write.
    let other = "import { fmt } from '../lib/fmt';\n";
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    mk(root, "src/app.ts", other)?;
    mk(root, "lib/fmt.ts", "export const fmt = 1;\n")?;
    let err = apply(root, &plan(), GitMode::Disabled).expect_err("span mismatch");
    assert!(
        matches!(err, jmove::core::JmoveError::StaleIndex(_)),
        "{err}"
    );
    assert_eq!(fs::read_to_string(root.join("src/app.ts"))?, other);
    assert!(root.join("lib/fmt.ts").exists());
    Ok(())
}

#[test]
fn dir_apply_prunes_emptied_source_dirs_last() -> JmoveResult<()> {
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    mk(root, "src/a.ts", "x\n")?;
    mk(root, "src/nested/b.ts", "y\n")?;
    let plan = MovePlan {
        source: "src".into(),
        target: "lib".into(),
        moves: vec![
            FileMove {
                source: "src/a.ts".into(),
                target: "lib/a.ts".into(),
            },
            FileMove {
                source: "src/nested/b.ts".into(),
                target: "lib/nested/b.ts".into(),
            },
        ],
        rewrites: Vec::new(),
        left_behind: Vec::new(),
        // shallowest last on purpose: apply must prune deepest first.
        prune_dirs: vec!["src/nested".into(), "src".into()],
    };
    apply(root, &plan, GitMode::Disabled)?;
    assert!(root.join("lib/nested/b.ts").is_file());
    assert!(!root.join("src").exists(), "emptied source tree must go");
    Ok(())
}

#[test]
fn dir_apply_rolls_back_every_move_when_a_later_one_fails() -> JmoveResult<()> {
    // "trap" is a file, so the second move's parent can never exist:
    // the first move must be undone and rewritten importers restored.
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    mk(root, "src/a.ts", "a\n")?;
    mk(root, "src/b.ts", "b\n")?;
    mk(root, "trap", "I am a file\n")?;
    mk(root, "app.ts", "import './src/a';\n")?;
    let plan = MovePlan {
        source: "src".into(),
        target: "lib".into(),
        moves: vec![
            FileMove {
                source: "src/a.ts".into(),
                target: "lib/a.ts".into(),
            },
            FileMove {
                source: "src/b.ts".into(),
                target: "trap/b.ts".into(),
            },
        ],
        rewrites: vec![Rewrite {
            file: "app.ts".into(),
            span: 8..15,
            old_text: "./src/a".into(),
            new_text: "./lib/a".into(),
        }],
        left_behind: Vec::new(),
        prune_dirs: vec!["src".into()],
    };
    let err = apply(root, &plan, GitMode::Disabled).expect_err("ENOTDIR");
    assert!(matches!(err, jmove::core::JmoveError::Io(_)), "{err}");
    assert!(root.join("src/a.ts").is_file(), "first move undone");
    assert!(!root.join("lib").exists(), "created dirs removed");
    assert_eq!(
        fs::read_to_string(root.join("app.ts"))?,
        "import './src/a';\n"
    );
    Ok(())
}
