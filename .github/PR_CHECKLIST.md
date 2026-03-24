# PR Checklist

Use this checklist before opening or merging a change.

## TDD Gate

- [ ] Red step completed first (tests edited before implementation).
- [ ] Focused Red test command was run and failed (non-zero exit).
- [ ] Red failure reason was captured in notes/PR description.
- [ ] Green implementation was done only after observed Red failure.
- [ ] Focused Green test command was re-run and passed.

## Test Coverage Gate

- [ ] Every newly added function has a direct unit test.
- [ ] New function to test mapping is documented in PR description.
- [ ] New helpers in src and tests are not exempt from direct tests.

## Comment Coverage Gate

- [ ] Every newly added public function has a RustDoc comment.
- [ ] Every newly added private function has a concise purpose comment.

## Validation Gate

- [ ] cargo test --all passed.
- [ ] cargo clippy --all-targets -- -D warnings passed.

## Docs Gate

- [ ] README.md was updated for every user-visible change.
- [ ] README.md includes at least one section/example matching new behavior.
- [ ] tests/paths/README.md was updated if fixtures were added/removed/renamed.

## Final Review Gate

- [ ] User-facing errors and warnings use required prefixes (error:, warning:).
- [ ] No user-facing unwrap usage was introduced.
- [ ] Change is not marked done if any gate above is unchecked.
