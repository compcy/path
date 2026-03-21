# Copilot Instructions

<!-- Instructions here apply to all Copilot interactions in this workspace. -->

## Language & Style

- All code should follow Rust idioms and best practices. Prefer iterators and functional style over explicit loops where it leads to clearer code. Use descriptive variable and function names to enhance readability.
- All code should be formatted using `rustfmt` to ensure a consistent style across the project. Adhere to the Rust API guidelines for public interfaces, and include documentation comments for all public items.
- RustDoc comments should be used to document public functions, structs, and modules. Private items should have inline comments explaining their purpose and any non-obvious implementation details. Avoid adding docstrings to unchanged functions to minimize noise in documentation updates.
- All code should pass Clippy lints with `cargo clippy --all-targets -- -D warnings`. Address all warnings and errors reported by Clippy to maintain code quality and consistency.

## Error Handling

- Prefix all user-facing error messages with `error:` and warnings with `warning:`.
- Fatal errors should call `std::process::exit(1)` after printing the message to stderr with `eprintln!`.
- Non-fatal issues (e.g. missing store file when not required) should warn and continue rather than exit.

## Testing

- Use red/green testing to ensure that all new code is covered by tests.
- Every new function should have a corresponding unit test.
- Integration tests should be created to test the output of the CLI commands.
- Be comprehensive in testing edge cases, such as invalid input formats, missing files, and permission issues.
- Test malicious input scenarios, such as attempts to store or load paths with shell metacharacters or escape sequences.
- Run all tests with `cargo test --all` to ensure they pass before committing changes.

## Project Conventions

- The file format for stored paths is `'<location>' [name] (options)`, where `location` is the path being stored, `name` is an optional identifier for the path, and `options` can include `auto`, `noauto`, `pre`, and `protect`.
- `README.md` should be updated to reflect any changes to the file format or CLI usage. Use clear sections with bullet points and code blocks; avoid large blocks of prose.
- Stored locations must be absolute, canonical-looking paths (no `.`/`..`, no trailing slash except `/`, no `:` or shell metacharacters).
- The store file always begins with the header comment: `# layout: '<location>' [<name>] (<options>)`.
- `path list` output uses unquoted locations: `<location> [<name>] (<options>)`.

## What to Avoid

- Do not add docstrings to unchanged functions.
- Do not use `unwrap()` on user-facing operations; handle errors explicitly.
- Do not persist system paths (e.g. `/bin`, `/usr/bin`) to the store file; they are managed by `path restore`.
- Do not use shell metacharacters or escape sequences in stored locations.
