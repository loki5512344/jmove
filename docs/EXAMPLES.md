# jmove — examples

Concrete examples for both audiences: humans at a terminal and AI agents
consuming `--json`. All outputs below are captured from the real binary.

Flags (see `jmove --help`): `mv <source> <target> [--dry-run] [--json]
[--no-git]`, `check [--json]`, and the global `--root <DIR>` /
`--source-root <DIR>` (monorepo subtree filter). `<source>` may also be a
directory (mirrored batch move, emptied dirs pruned). Inside a git repo, `mv` of a tracked file uses `git mv` (the
rename is staged); `--no-git` forces a plain filesystem rename.
Exit codes: `0` ok · `1` operation error · `2` `check` found broken imports.

## Human usage

### Preview a move (always do this first)

```console
$ jmove mv lib/sum.ts utils/sum.ts --dry-run
--- app.ts
+++ app.ts
@@ -1,4 +1,4 @@
-import { sum } from "./lib/sum";
+import { sum } from "./utils/sum";

 export function main(): number {
   return sum(1, 2);
move lib/sum.ts -> utils/sum.ts
```

Nothing on disk changed. If no file imports the source, the diff is empty.

### Apply the move

```console
$ jmove mv lib/sum.ts utils/sum.ts
moved lib/sum.ts -> utils/sum.ts, updated 1 import in 1 file
```

Rewrites happen first, the rename last; any failure rolls everything back.

### Verify the project afterwards

```console
$ jmove check
check: no findings
```

When something does point at nothing, `check` prints one line per broken
import and exits with code `2`:

```console
$ jmove check
src/broken.ts:4: cannot resolve './gone'
$ echo $?
2
```

### Operating on another project

`--root` points jmove at a project other than the current directory; all
path arguments stay relative to that root:

```console
$ jmove --root ~/code/frontend mv src/old.ts src/new.ts --dry-run
```

### Errors

```console
$ jmove mv lib/sum.ts app.ts
jmove: target path 'app.ts' already exists
  hint: remove or rename the existing target first
$ echo $?
1
```

jmove never overwrites: free the destination (or pick another name) and
retry. A missing source reports `SOURCE_NOT_FOUND` the same way, and a
file nobody imports simply moves with zero rewrites.

### Java: package + imports + move in one step

Java couples the `package` declaration to the directory layout, so a move
is three coordinated edits — jmove makes all of them:

```console
$ jmove mv src/main/java/com/example/util/Text.java src/main/java/com/example/core/Text.java --dry-run
--- src/main/java/com/example/app/App.java
+++ src/main/java/com/example/app/App.java
@@ -1,7 +1,7 @@
 package com.example.app;

-import com.example.util.Text;
-import static com.example.util.Text.shout;
+import com.example.core.Text;
+import static com.example.core.Text.shout;
...
move src/main/java/com/example/util/Text.java -> src/main/java/com/example/core/Text.java
$ jmove mv src/main/java/com/example/util/Text.java src/main/java/com/example/core/Text.java
moved src/main/java/com/example/util/Text.java -> src/main/java/com/example/core/Text.java, updated 4 imports in 3 files
```

The `3 files` include the moved file itself: its `package` line is the
fourth rewrite. Targets outside the source root, non-`.java` targets and
cross-directory moves of default-package classes are rejected with
`PLAN_REJECTED` (exit 1) and change nothing.

### Fix: auto-repair unused imports

`fix` runs deterministic rules and applies them through the same
dry-run/atomic engine. Preview first, then apply:

```console
$ cat src/main/java/com/example/app/App.java
package com.example.app;

import com.example.Text;
import com.example.unused.Ghost;   // never referenced

...
$ jmove fix --dry-run
--- src/main/java/com/example/app/App.java
+++ src/main/java/com/example/app/App.java
@@ -1,6 +1,5 @@
 package com.example.app;

 import com.example.Text;
-import com.example.unused.Ghost;
...
$ jmove fix
fixed 1 issue in 1 file
```

Only provably-dead imports are removed: the whole statement line (with its
newline) disappears and every other line stays byte-identical. A name that
also appears in a comment, string literal or a sibling static import is
kept, so the rule can only under-report, never delete live code. Scope to
one rule with `--rule java/unused-import` (`java/import-order` sorts the block
google-style); an unknown id exits `1` with `INVALID_ARGUMENT` and lists the rules.

### Fix: add a missing import for a bare type reference

After a Java `mv` changes a file's package, references to former
same-package siblings stop resolving. `java/missing-import` inserts the
import for a unique FQN in the class index; ambiguous ones are reported.

```console
$ jmove fix --rule java/missing-import
fixed 1 issue in 1 file   # inserted: import com.example.util.Maths;
```

## AI-agent usage (`--json`)

Every `--json` response is a flat envelope: `status` (`"ok"` | `"dry_run"`
| `"error"`) and `operation` plus the payload fields. Errors carry a
stable `code` and an actionable `hint`.

### 1. Preview

```console
$ jmove mv lib/sum.ts utils/sum.ts --dry-run --json
{
  "status": "dry_run",
  "operation": "mv",
  "would_move": "lib/sum.ts",
  "target": "utils/sum.ts",
  "would_update": 1,
  "affected_files": [
    "app.ts"
  ],
  "would_move_via": "fs",
  "diff": "--- app.ts\n+++ app.ts\n@@ -1,4 +1,4 @@\n-import { sum } from \"./lib/sum\";\n+import { sum } from \"./utils/sum\";\n..."
}
```

Review `affected_files`; abort and ask the user if the blast radius is
unexpected.

### 2. Apply

```console
$ jmove mv lib/sum.ts utils/sum.ts --json
{
  "status": "ok",
  "operation": "mv",
  "source": "lib/sum.ts",
  "target": "utils/sum.ts",
  "changed_files": [
    {
      "path": "app.ts",
      "changes": [
        {
          "line": 1,
          "old": "./lib/sum",
          "new": "./utils/sum"
        }
      ]
    }
  ],
  "moved": 1,
  "updated_imports": 1,
  "moved_via": "fs"
}
```

`changed_files[].changes[]` lists every rewritten specifier with its
1-based line; `moved` and `updated_imports` are the counters,
`moved_via` tells whether the rename went through git (`"git"`, staged)
or the plain filesystem (`"fs"`).

### 3. Verify

```console
$ jmove check --json
{
  "status": "ok",
  "operation": "check",
  "broken_imports": [
    {
      "file": "src/broken.ts",
      "line": 4,
      "import": "./gone",
      "reason": "file_not_found"
    }
  ],
  "total": 1
}
```

Note: this response keeps `status: "ok"` (the command itself succeeded)
while the process exits `2`; treat a non-zero `total` — or exit code `2` —
as a failed refactor. A clean project returns `"broken_imports": [], "total": 0`
and exit code `0`.

### Error shape

```console
$ jmove mv lib/sum.ts app.ts --json
{
  "status": "error",
  "operation": "mv",
  "code": "TARGET_EXISTS",
  "message": "target path 'app.ts' already exists",
  "hint": "remove or rename the existing target first"
}
```

Stable codes: `TARGET_EXISTS`, `SOURCE_NOT_FOUND`, `INVALID_ARGUMENT`,
`IO_ERROR`, `STALE_INDEX`, `PLAN_REJECTED` (exit `1`). The recommended
agent loop — preview, inspect, apply, verify — is in `docs/SKILL.md`.
