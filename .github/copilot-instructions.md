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
  1. **Red** — Write the tests first. Run `cargo test --all` and confirm the new tests _fail_ before writing any implementation code. Do not proceed until the failure is verified.
  2. **Green** — Write the minimum implementation needed to make the failing tests pass. Run `cargo test --all` again and confirm all tests now pass.
- Mandatory execution gate for each behavior change:
  1. Edit only test files (and fixtures/docs for tests) in Red.
  2. Run a focused failing test command (for example `cargo test --test cli <test_name>`), capture the failure, and report it.
  3. Only after observing that failure, edit implementation files under `src/`.
  4. Re-run the focused test(s), then `cargo test --all`, then `cargo clippy --all-targets -- -D warnings`.
- Implementation edits before an observed Red failure are not allowed. If this happens, stop, disclose the deviation, and re-run the workflow from Red.
- Never write implementation code and its tests in the same step; the failing-test state must be observed and confirmed between steps.
- This rule applies to all production code and all test-only code, including test helpers, test utilities, parsing helpers used only by tests, and shared fixture/setup helpers.
- Adding or modifying helper functions used by tests is not an exception to TDD: write the helper tests first, observe the failing state, and only then implement or modify the helper.
- Do not batch multiple new helper implementations into a single Green step unless each helper's corresponding failing test state was already observed and recorded during a prior Red step.
- When reporting work, explicitly indicate the observed **Red** failure before the **Green** implementation step so the workflow is auditable in the conversation history.
- In status updates and final summaries, include exact commands used for Red and Green and whether each command exited non-zero (Red) or zero (Green).
- Every new function must have a corresponding direct unit test written first in the Red step, before any implementation edits.
- Refactors are not exempt: when a refactor introduces any new function (including private helpers), add corresponding unit tests for each new function in the same change.
- If any new function lacks a direct test, the change is incomplete and must not be reported as done.
- If any new function lacks the required purpose comment/doc comment, the change is incomplete and must not be reported as done.
- Integration tests should be created to test the output of the CLI commands.
- Be comprehensive in testing edge cases, such as invalid input formats, missing files, and permission issues.
- Test malicious input scenarios, such as attempts to store or load paths with shell metacharacters or escape sequences.
- Run all tests with `cargo test --all` to ensure they pass before committing changes.

## Defect Fix Methodology

- For bug fixes, first reproduce the defect with a focused unit test (or integration test when behavior is CLI-only) before changing implementation code.
- Record and verify the failing Red state by running the relevant test command and confirming the new test fails for the expected reason.
- For defects involving `.path` parsing or validation behavior, add or update dedicated fixture files in `tests/paths/*.path` and prefer `copy_fixture_to_temp_store` in integration tests instead of inline `fs::write` fixture content.
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
- When adding or renaming test fixtures, keep fixture documentation synchronized (for example `tests/paths/README.md`) and add or update tests that fail when docs and fixture files diverge.
- Stored locations must be absolute, canonical-looking paths (no `.`/`..`, no trailing slash except `/`, no `:` or shell metacharacters).
- The store file always begins with the header comment: `# layout: '<location>' [<name>] (<options>)`.
- `path list` output uses unquoted locations: `<location> [<name>] (<options>)`.

## What to Avoid

- Do not add docstrings to unchanged functions.
- Do not use `unwrap()` on user-facing operations; handle errors explicitly.
- Do not persist system paths (e.g. `/bin`, `/usr/bin`) to the store file; they are managed by `path restore`.
- Do not use shell metacharacters or escape sequences in stored locations.
