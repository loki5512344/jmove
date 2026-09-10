# jmove — Product Plan

## Vision

The `mv` for source code that never breaks imports: fast, safe (dry-run +
atomic rollback), single static Rust binary — usable by humans in a
terminal and by AI agents over `--json`, with zero external toolchains.

## Competitive landscape

The closest analog is **refac / ai_refac** (github: jav-ed/ai_refac):
moves sources and updates imports/paths across the project, aimed at
AI agents without an IDE.

| | refac/ai_refac | jmove |
|---|---|---|
| Languages | TS/JS (ts-morph+Bun), Python (Rope), Rust (rust-analyzer LSP), Go (gopls), Dart, Markdown | TS/JS → Java → Python/Go |
| Under the hood | spawns external toolchains/LSPs per run | pure Rust, tree-sitter grammars in-process |
| Dry-run | **none** — writes straight to disk | core feature, unified diff |
| Atomicity/rollback | not described | rewrite-then-move, rollback on failure |
| TS limit | 30 files per call | none (index-based) |
| Go behaviour | moves whole package for any file | per-file moves |
| Java | not supported, not planned | **open niche — headline feature** |
| UX | agent-oriented CLI | human UX + first-class `--json` agent mode |

**Conclusion:** the idea is validated, the product is not. jmove wins on
dry-run, speed (no LSP startup), human UX, single-binary distribution,
the `split` command (nobody has it) and Java.

## Why Java is the wedge

Java imports are fully-qualified package paths hard-coupled to directory
layout:

```
src/com/example/utils/Parser.java
→ package com.example.utils;           // must change
→ import com.example.utils.Parser;     // must change in every importer
→ + physical file move
```

Three coordinated edits per move; outside IntelliJ nobody automates it.
In headless/CI/agent contexts there is no IntelliJ. Implementation:
tree-sitter Java grammar (mature) + directory⇄package convention +
source-root detection (`src/main/java`, `src/`, ...). This is the feature
that makes jmove *the* answer for "move a Java file in CI".

## AI-agent design

Existing tools ship a raw `.agents/skills/` folder; we do it properly:

- `--json` on every command. Envelope:
  `{ "status": "ok" | "dry_run" | "error", "operation": "...", ... }`
- Stable error codes + actionable hints:
  `{ "status":"error", "code":"TARGET_EXISTS", "message":"...", "hint":"choose another target or remove the existing one" }`
- `mv --dry-run --json` → `{ would_move, would_update, affected_files }`
  so agents preview → confirm → apply → `check --json`.
- Exit codes: `0` ok · `1` error · `2` broken imports (from `check`).
- `docs/SKILL.md` — machine-readable instructions an agent can drop into
  context: short, no fluff, workflow-first.

## Correctness principles

1. Rewrite **only the specifier string** (byte span), never reformat the
   statement — no formatter dependency, minimal diffs.
2. Resolution must match the language's own lookup: TS extension guessing
   + `index.*`; Java package⇄path; (Phase 2) tsconfig `paths` aliases.
3. Imports are rewritten **before** the move; any error rolls back all
   writes. Prefer `git mv` when inside a repo to preserve history.
4. Beyond imports, warn (don't silently rewrite) about other references:
   `package.json` exports, jest mocks, `tsconfig` includes, markdown links.

## Phases

- **Phase 1 (MVP)** — as in `todo.md`: TS/JS scan+index, graph, mv with
  dry-run/atomic apply. Add to MVP: relative-path resolution incl.
  `index.*` (without it the tool is toy-grade); `check` as the self-test.
- **Phase 1.5 — Java.** The differentiator, as soon as the TS pipeline is
  proven. Also: `--json` + SKILL.md ship *with* Java, not after.
- **Phase 2** — disk index cache (`.jmove/`), incremental reindex,
  tsconfig paths, rayon parallelism, `--git` integration.
- **Phase 3** — Python, Go (per-file, unlike refac), `split` command.
- **Later** — LSP server mode (become the thing refac spawns), watch mode,
  VS Code wrapper.

## Rules (binding, see todo.md)

KISS · DRY · SOLID · ≤250 lines/file · ≤4 files/folder · fmt+clippy
`-D warnings` per commit · no dead code, no `#[allow(dead_code)]` ·
English Javadoc-style comments on all public API.
