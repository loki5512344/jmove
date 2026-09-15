# jmove — TODO

## Правила
- KISS — simplest solution that works; no speculative abstractions.
- DRY — no duplicated logic; extract into a function/module instead.
- SOLID — single responsibility per module (`cli`/`core`/`parser`/`cache`), open for extension (new languages) via the parser trait, no deps on implementation details.
- Max 250 lines per file — split when exceeded.
- Max 4 files per folder (module) — split the module when exceeded.
- Каждый коммит проходит `cargo fmt --check` и `cargo clippy -- -D warnings`.
- Запрещён mёртвый код: unused code удаляется или реализуется; `#[allow(dead_code)]` не использовать.

## MVP (Phase 1)
- [x] CLI skeleton (clap) — команды: mv, check, --dry-run, --json
- [x] Сканер файлов проекта (уважать .gitignore через `ignore` крейт)
- [x] Парсер импортов для TypeScript/JS через tree-sitter
- [x] Разрешение путей: extension guessing (.ts/.tsx/.js/...), index.* (в MVP — без этого инструмент игрушка)
- [x] Построение графа зависимостей (файл → что импортирует)
- [x] Инвертированный граф (файл → кто его импортирует)
- [x] Вычисление нового относительного пути после mv
- [x] Rewrite импортов в файлах (трогаем только specifier-строку, никогда не реформатим statement)
- [x] Атомарный apply (сначала rewrite, потом mv) + rollback при ошибке
- [x] Dry-run режим с diff выводом
- [x] Команда check (битые импорты, exit code 2) — self-test инструмента
- [x] Тесты: unit (parser, core) + e2e (CLI на фикстурах)

## AI / Agent support
- [x] --json флаг на всех командах (status ok|dry_run|error)
- [x] --dry-run + --json (preview без записи на диск)
- [x] Стабильные error codes (TARGET_EXISTS, SOURCE_NOT_FOUND, ...) + hint поле
- [x] Exit codes: 0 ok / 1 error / 2 broken imports
- [x] docs/SKILL.md — машиночитаемая документация для AI агентов
- [x] docs/EXAMPLES.md — примеры для людей и агентов

## Phase 1.5 — Java (наша ниша, аналогов в CLI нет)
- [x] tree-sitter Java грамматика: package + import extraction
- [x] Детект source root (src/main/java, src/) и соответствие package ⇄ директория
- [x] mv = три синхронных правки: package, все import в проекту, физический перенос
- [x] e2e фикстуры tests/java/

## Phase 1.6 — fix (auto-fix мелких ошибок; тот же safety-движок, что mv)
Мотивация: jmove уже умеет index → план правок → dry-run diff → atomic apply →
--json. Превращаем «mv» в обобщённый «найти правки → применить безопасно».
Слоган: **mv + fix, один движок правок, два генератора планов**.
Решение: паттерны (вариант 2) — ДА; ML (вариант 1) — НЕТ (детерминизм = продукт:
dry-run/rollback/error-codes не переживают недетерминированный движок).
AI оставляем СНАРУЖИ: при неоднозначности jmove отдаёт `candidates` в --json,
агент (LLM) выбирает и повторяет команду — в рамках нашего agent-first UX.
- [x] Edit engine: `core::Edit{span,old_text,new_text}` — replace/insert/delete в одном движке
      (`rewrite_bytes` generic над `&[Edit]`, пустой span = вставка, `new_text=""` = удаление;
      overlap/malformed spans → PlanRejected, не-char-boundary/content-mismatch → StaleIndex
      до записи); apply/rollback/diff/json переведены минимально (MovePlan.rewrites → Edit через `From`)
- [x] `Fix` trait рядом с `Language`: `fixes(path, source, index) -> Vec<FixCandidate>`
      {rule, severity, auto_fixable, edits}; `jmove fix [--rule ...] [--dry-run] [--json]`
      (тот же apply/rollback/diff: `apply_edits` + `render_edits_diff` без move)
- [~] Java v1: unused-imports (DONE, skip wildcard/ambiguous), missing-import (DONE: unique FQN candidate →
      insert `import pkg.Type;` at the import-block end; ambiguous/wildcard → `--json` `candidates`, applied:false;
      закрыт guava-разрыв «перенесли файл, соседняя ссылка без импорта умерла» — проверено mv+fix+javac SUCCESS),
      import-order (DONE: Google-стиль — statics первыми, ASCII-сортировка, дедуп; конфликтующие с другими правилами откладываются (prune_overlaps по severity) и сходятся за 2-3 прогона),
 class-name-mismatch (DONE как ПОВЕРХНОСТЬ check, не fix: починка = переименование файла, а fix-движок умеет только байтовые правки;
 check отдаёт находку с готовой командой `jmove mv`, exit code 2; rename не меняет FQN → импорты не трогаются)
- [x] TS v1: unused-imports (DONE: whole-statement delete, все биндинги мертвы → строка уходит;
      mixed used/unused НЕ трогаем — в ESM импорт исполняет побочные эффекты модуля,
      partial-удаление specifier'ов отложено осознанно)
- [ ] TS v1: import-order (нет кэнона без eslint-config — grouping-конвенции плавающие; ждать запроса),
      add-import требует индекс экспортов (символ→файл)
- [ ] Форматирование: свой cargo-fmt НЕ строим (вечный long-tail). Только «import formatting»
      (порядок/группировка — у нас уже есть spans). Опционально `--format-after <cmd>` (prettier /
      google-java-format), не зависимость
- [ ] Интероп PMD/Checkstyle/eslint (фаза 2.5): `jmove fix --report checkstyle.xml` маппит
      violation(file,line,rule) на паттерны; на выход SARIF для CI/IDE.
      Маркетинг: «auto-fix for what Checkstyle only reports»

## Guava real-world smoke test (google/guava @ main, JDK21, mvnw) — ПРОВЕРЕНО
- [x] mv Primitives primitives→util: 5 правок (4 imports + package), `mvn -pl guava compile`
      = BUILD SUCCESS, `jmove check` чисто
- [x] mv VisibleForTesting annotations→annotations.testing (63 файла): jmove переписал все 62
      явных импорта корректно, НО javac упал: сам перенесённый файл ссылался на соседний
      `GwtCompatible` БЕЗ импорта (тот же пакет) → после mv ссылка битая. jmove в v1 осознанно
      НЕ добавляет импорты. Это главный driver для fix/missing-import из Phase 1.6 выше
- [x] Обе перемещения повторены как `mv` + авто-`fix`, compile = BUILD SUCCESS (guava main, JDK21):
      VisibleForTesting annotations→annotations.testing: mv без --source-root давал 1 правку (62 потеряны!),
      с --source-root guava — 63/63 через git mv; fix добавил в перенесённый файл
      `import com.google.common.annotations.GwtCompatible` (тот самый разрыв v1) + 36 import-order
      (сошёлся за 2 прогона); Primitives primitives→util: 5 правок + fix; `jmove check` = 0 broken
- [x] Индексация в monorepo с дублями пакетов — ПРОВЕРЕНО НА guava (см. выше):
      глобальный `--source-root DIR` — индексирует (mv/check/fix) только поддерево,
      FQN-коллизии исчезают, соседнее дерево не трогается; авто-определение по mv-цели
      осознанно НЕ делаем (явный флаг предсказуемее, см. KISS)

## Phase 2
- [ ] Кэш индекса на диске (bincode/rkyv) → .jmove/index
- [ ] Инкрементальная переиндексация (только изменённые файлы)
- [x] Поддержка tsconfig paths / алиасов (@/...): JSONC-парсер (комментарии/хвостовые запятые),
      baseUrl + star/exact keys, longest-prefix wins; при mv алиас сохраняется, если файл остался
      в дереве алиаса, иначе fallback на относительный; extends/2+ кандидаты — осознанно не делаем (v1)
- [ ] Параллельная индексация через rayon
- [x] --git интеграция (git mv для stage/истории): auto для tracked файлов, --no-git флаг, moved_via/would_move_via в --json
- [x] Перенос директорий целиком (mv папки): зеркальный batch-move всех индексируемых файлов, merged rewrites, prune пустых исходных каталогов, left_behind для неиндексируемых
- [ ] Предупреждения о не-import ссылках: package.json exports, jest mocks, tsconfig includes, markdown links
- [ ] prettier интеграция после rewrite (по желанию)

## Phase 3
- [ ] Поддержка Python (from/import, относительные точки)
- [ ] Поддержка Go (per-file, НЕ whole-package как refac)
- [ ] Команда split (авто-разбивка файла на несколько)
- [x] Windows-пути: `core::rel_str` — единый формат относительных путей на границе CLI
      (human/JSON/diff-заголовки/git-pathspecs всегда через `/`, не `Path::display()`);
      CI matrix linux+windows (`cargo test --locked`), checkout с `core.autocrlf=input`.
      Camino не ввели: PathBuf остаётся внутренней валютой, славши нужен только на выводе

## Идеи на потом
- [ ] LSP интеграция (jmove сам как LSP server)
- [ ] Watch mode
- [ ] VS Code расширение как обёртка над CLI

## Конкуренты (см. docs/PLAN.md)
- refac / ai_refac (jav-ed): TS/Py/Rust/Go/Dart, но без dry-run, лимит 30 файлов в TS,
  Go = весь пакет, Java нет. Наш edge: dry-run+атомарность, Java, split, скорость (без LSP), UX.
