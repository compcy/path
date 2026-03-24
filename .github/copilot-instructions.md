# Copilot Instructions

<!-- Instructions here apply to all Copilot interactions in this workspace. -->

## Language & Style

- All code should follow Rust idioms and best practices. Prefer iterators and functional style over explicit loops where it leads to clearer code. Use descriptive variable and function names to enhance readability.
- All code should be formatted using `rustfmt` to ensure a consistent style across the project. Adhere to the Rust API guidelines for public interfaces, and include documentation comments for all public items.
- RustDoc comments should be used to document public functions, structs, and modules. Private items should have inline comments explaining their purpose and any non-obvious implementation details. Avoid adding docstrings to unchanged functions to minimize noise in documentation updates.
- Every newly added function must include a concise purpose comment at creation time.
- Public functions must use RustDoc comments (`///`), and private functions must have a brief inline comment directly above the function.
- Newly added helper functions, including test helpers and shared test utilities in `tests/`, are not an exception and must also have this comment coverage.
- All code should pass Clippy lints with `cargo clippy --all-targets -- -D warnings`. Address all warnings and errors reported by Clippy to maintain code quality and consistency.

## Error Handling

- Prefix all user-facing error messages with `error:` and warnings with `warning:`.
- Fatal errors should call `std::process::exit(1)` after printing the message to stderr with `eprintln!`.
- Non-fatal issues (e.g. missing store file when not required) should warn and continue rather than exit.

## Testing

- Always use red/green TDD for all development and behavior changes, without exception.
- Follow red/green TDD strictly as two separate steps:
  1. **Red** — Write the tests first. The code must compile and the new tests must _run_ but _fail_ on their assertions before writing any implementation code. A compile error (e.g. "function not found") is **not** a valid Red state — the test must compile, execute, and fail at an assertion. Do not proceed until a genuine test failure is verified.
  2. **Green** — Write the minimum implementation needed to make the failing tests pass. Run `cargo test --all` again and confirm all tests now pass.
- **Red always requires a running assertion failure, not a compile error.** When new tests reference functions that do not yet exist, first add a minimal non-functional stub in `src/` (e.g. a function body containing `todo!()` or `unimplemented!()`) so the project compiles. Only after the project compiles should you run the tests and observe the assertion failure. The stub is not considered implementation — it exists solely to allow the code to compile and the test to reach its assertion.
- Mandatory execution gate for each behavior change:
  1. Edit test files (and fixtures/docs for tests) in Red. If the new tests reference functions that do not yet exist, also add minimal stubs in `src/` so the project compiles.
  2. Run a focused failing test command (for example `cargo test --test cli <test_name>`), capture the **assertion failure** output, and report it. A compiler error is not an acceptable Red state — add stubs until it compiles, then run.
  3. Only after observing a running test failure, replace stubs with real implementation in `src/`.
  4. Re-run the focused test(s), then `cargo test --all`, then `cargo clippy --all-targets -- -D warnings`.
- Implementation edits before an observed Red failure are not allowed. If this happens, stop, disclose the deviation, and re-run the workflow from Red.
- Never write implementation code and its tests in the same step; the failing-test state must be observed and confirmed between steps.
- This rule applies to all production code and all test-only code, including test helpers, test utilities, parsing helpers used only by tests, and shared fixture/setup helpers.
- Adding or modifying helper functions used by tests is not an exception to TDD: write the helper tests first, observe the failing state, and only then implement or modify the helper.
- Do not batch multiple new helper implementations into a single Green step unless each helper's corresponding failing test state was already observed and recorded during a prior Red step.
- When reporting work, explicitly indicate the observed **Red** failure before the **Green** implementation step so the workflow is auditable in the conversation history.
- The Red failure reported must be the **test assertion output** (e.g. `thread 'test_name' panicked at ...`), not a compiler diagnostic. If only a compiler error was observed, the Red gate has not been satisfied.
- In status updates and final summaries, include exact commands used for Red and Green and whether each command exited non-zero (Red) or zero (Green).
- Every new function must have a corresponding direct unit test written first in the Red step, before any implementation edits.
- Refactors are not exempt: when a refactor introduces any new function (including private helpers), add corresponding unit tests for each new function in the same change.
- If any new function lacks a direct test, the change is incomplete and must not be reported as done.
- If any new function lacks the required purpose comment/doc comment, the change is incomplete and must not be reported as done.
- Integration tests should be created to test the output of the CLI commands.
- Integration tests that exercise store-file parsing, validation, diagnostics, or sanitized rendering must use `.path` fixtures under `tests/paths/` and `copy_fixture_to_temp_store`; do not inline store-file payloads with `fs::write` in those tests.
- This fixture-first rule also applies to malicious input coverage (control characters, invisible Unicode, shell metacharacters, delimiter edge-cases, and malformed field shapes).
- Inline `fs::write` store-file content is allowed only when a test must generate content dynamically and cannot be represented as a static fixture; such cases must include a short comment explaining why a fixture is not suitable.
- Be comprehensive in testing edge cases, such as invalid input formats, missing files, and permission issues.
- Test malicious input scenarios, such as attempts to store or load paths with shell metacharacters or escape sequences.
- Run all tests with `cargo test --all` to ensure they pass before committing changes.

## Defect Fix Methodology

- For bug fixes, first reproduce the defect with a focused unit test (or integration test when behavior is CLI-only) before changing implementation code.
- Record and verify the failing Red state by running the relevant test command and confirming the new test **compiles, runs, and fails** for the expected reason (assertion failure). A compile error does not satisfy this gate.
- For defects involving `.path` parsing or validation behavior, add or update dedicated fixture files in `tests/paths/*.path` and prefer `copy_fixture_to_temp_store` in integration tests instead of inline `fs::write` fixture content.
- For defects involving echoed diagnostics or output sanitization from `.path` content, add/update dedicated fixtures in `tests/paths/*.path` (including malicious/control-character cases) rather than embedding those payloads inline in tests.
- When fixture files are added, removed, or renamed, update `tests/paths/README.md` in the same change.
- Implement the smallest possible fix that addresses the observed failure; avoid broad refactors unless required for correctness.
- Re-run the focused tests first, then run `cargo test --all`, and finally run `cargo clippy --all-targets -- -D warnings`.
- In status updates and final summaries, report defect fixes in explicit Red then Green order so the workflow is auditable.

## Project Conventions

- The file format for stored paths is `'<location>' [name] (options)`, where `location` is the path being stored, `name` is an optional identifier for the path, and `options` can include `auto`, `noauto`, `pre`, and `protect`.
- All names must be unique and cannot contain whitespace or shell metacharacters. They should be descriptive of the path's purpose (e.g. `cargo`, `pipx`).
- `README.md` must be updated in the same change for any user-visible change, including: CLI behavior, command output, warnings/errors, defaults, examples, flags, and file-format semantics.
- `README.md` update is mandatory even for "small" UX changes (for example output order, warning timing, fallback behavior, or default-mode behavior).
- Before reporting work complete, verify that each user-visible change has matching README text and at least one example/section reflecting the new behavior.
- If implementation changed and README was not updated where needed, the change is incomplete and must not be reported as done.
- The `README.md` follows a defined section order — do not reorganize it; only add content within the appropriate section:
  1. **Introduction** — one-paragraph description and sample `path` table output.
  2. **Installation** — Cargo install, direct build, and Shell Integration (wrapper setup).
  3. **Simple Usage** — ephemeral commands that do not write to the store file (`path`, `path add` without a name, `path remove`).
  4. **Stored Entries** — commands that read or write the store file (`path add <name>`, `path list`, `path delete`, `path load`, `path verify`). The `path verify` callout must stay in this section.
  5. **Manual File Editing** — store file format, valid options, escaping rules. Must always include a prominent warning to run `path verify` after editing.
  6. **Using a Specific Store File** — the `--file` global option and when to use it.
  7. **Restoring System Paths** — `path restore` and the list of restored paths.
  8. **Store Validation Rules** — what makes an entry invalid and how the default `path` command differs from strict commands.
  9. **Command Reference** — summary table of all commands.
  10. **CI Checks** — local commands to reproduce CI.
  11. **License**
- Keep individual sections concise; prefer short paragraphs and illustrative code blocks over long bullet lists.
- When adding a new command, add it to **Command Reference** (section 9) and to the most relevant numbered section above.
- When adding or renaming test fixtures, keep fixture documentation synchronized (for example `tests/paths/README.md`) and add or update tests that fail when docs and fixture files diverge.
- Stored locations must be absolute, canonical-looking paths (no `.`/`..`, no trailing slash except `/`, no `:` or shell metacharacters).
- The store file always begins with the header comment: `# layout: '<location>' [<name>] (<options>)`.
- `path list` output uses unquoted locations: `<location> [<name>] (<options>)`.

## What to Avoid

- Do not add docstrings to unchanged functions.
- Do not use `unwrap()` on user-facing operations; handle errors explicitly.
- Do not persist system paths (e.g. `/bin`, `/usr/bin`) to the store file; they are managed by `path restore`.
- Do not use shell metacharacters or escape sequences in stored locations.
