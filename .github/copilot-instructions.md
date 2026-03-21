# Copilot Instructions

<!-- Instructions here apply to all Copilot interactions in this workspace. -->

## Language & Style

- All code should follow Rust idioms and best practices. Prefer iterators and functional style over explicit loops where it leads to clearer code. Use descriptive variable and function names to enhance readability.
- All code should be formatted using `rustfmt` to ensure a consistent style across the project. Adhere to the Rust API guidelines for public interfaces, and include documentation comments for all public items.
- RustDoc comments should be used to document public functions, structs, and modules. Private items should have inline comments explaining their purpose and any non-obvious implementation details. Avoid adding docstrings to unchanged functions to minimize noise in documentation updates.
- Newly added helper functions, including test helpers and shared test utilities in `tests/`, should have a concise comment describing their purpose even when the implementation is short.
- All code should pass Clippy lints with `cargo clippy --all-targets -- -D warnings`. Address all warnings and errors reported by Clippy to maintain code quality and consistency.

## Error Handling

- Prefix all user-facing error messages with `error:` and warnings with `warning:`.
- Fatal errors should call `std::process::exit(1)` after printing the message to stderr with `eprintln!`.
- Non-fatal issues (e.g. missing store file when not required) should warn and continue rather than exit.

## Testing

- Follow red/green TDD strictly as two separate steps:
  1. **Red** — Write the tests first. Run `cargo test --all` and confirm the new tests _fail_ before writing any implementation code. Do not proceed until the failure is verified.
  2. **Green** — Write the minimum implementation needed to make the failing tests pass. Run `cargo test --all` again and confirm all tests now pass.
- Never write implementation code and its tests in the same step; the failing-test state must be observed and confirmed between steps.
- This rule applies to all production code and all test-only code, including test helpers, test utilities, parsing helpers used only by tests, and shared fixture/setup helpers.
- Adding or modifying helper functions used by tests is not an exception to TDD: write the helper tests first, observe the failing state, and only then implement or modify the helper.
- Do not batch multiple new helper implementations into a single Green step unless each helper's corresponding failing test state was already observed and recorded during a prior Red step.
- When reporting work, explicitly indicate the observed **Red** failure before the **Green** implementation step so the workflow is auditable in the conversation history.
- Every new function should have a corresponding unit test.
- Integration tests should be created to test the output of the CLI commands.
- Be comprehensive in testing edge cases, such as invalid input formats, missing files, and permission issues.
- Test malicious input scenarios, such as attempts to store or load paths with shell metacharacters or escape sequences.
- Run all tests with `cargo test --all` to ensure they pass before committing changes.

## Project Conventions

- The file format for stored paths is `'<location>' [name] (options)`, where `location` is the path being stored, `name` is an optional identifier for the path, and `options` can include `auto`, `noauto`, `pre`, and `protect`.
- All names must be unique and cannot contain whitespace or shell metacharacters. They should be descriptive of the path's purpose (e.g. `cargo`, `pipx`).
- `README.md` should be updated to reflect any changes to the file format or CLI usage. Use clear sections with bullet points and code blocks; avoid large blocks of prose.
- `README.md` should be updated to reflect any changes to the file format, CLI usage, or user-visible CLI output format (for example new columns, headers, markers, or field semantics in `path list --pretty`).
- When adding or renaming test fixtures, keep fixture documentation synchronized (for example `tests/paths/README.md`) and add or update tests that fail when docs and fixture files diverge.
- Stored locations must be absolute, canonical-looking paths (no `.`/`..`, no trailing slash except `/`, no `:` or shell metacharacters).
- The store file always begins with the header comment: `# layout: '<location>' [<name>] (<options>)`.
- `path list` output uses unquoted locations: `<location> [<name>] (<options>)`.

## What to Avoid

- Do not add docstrings to unchanged functions.
- Do not use `unwrap()` on user-facing operations; handle errors explicitly.
- Do not persist system paths (e.g. `/bin`, `/usr/bin`) to the store file; they are managed by `path restore`.
- Do not use shell metacharacters or escape sequences in stored locations.
