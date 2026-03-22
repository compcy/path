# Test Fixtures for `.path` Files

This directory contains pre-made `.path` store files for integration tests.

## Why Fixtures?

- Simplifies tests by removing inline file content
- Makes tests more readable
- Easy to add new test cases from bug reports
- Centralizes test data management

## Fixture Naming Convention

- `empty.path` - Empty store file (no entries)
- `single_entry.path` - One valid entry with auto flag
- `auto_noauto.path` - Two entries with different auto settings
- `protected_entry.path` - Entry with protect flag
- `two_entries.path` - Multiple valid entries
- `spaced_entry.path` - Path with spaces in location
- `duplicate_names.path` - Multiple entries with same name (error case)
- `duplicate_paths.path` - Multiple entries that normalize to the same stored location (error case)
- `legacy_unwrapped.path` - Legacy format without wrapping delimiters (error case)
- `one_invalid_one_valid.path` - Mix of valid and invalid locations (warning case)
- `system_paths.path` - Built-in system paths stored explicitly (allowed case)
- `known_paths.path` - Known non-system built-in paths stored explicitly (allowed case)

## Malicious Fixtures

The `malicious/` directory contains negative fixtures used by delimiter and
metacharacter hardening tests. These file names use the pattern
`<field>_<shape>.path` where field is one of `location`, `name`, or `options`.

- `location_ampersand.path` - location includes `&`
- `location_asymmetric_backtick.path` - location contains unmatched backtick shape
- `location_backtick.path` - location includes backtick
- `location_braces.path` - location includes `{}`
- `location_close_brace.path` - location includes `}`
- `location_close_bracket.path` - location includes `]`
- `location_close_parenthesis.path` - location includes `)`
- `location_dollar.path` - location includes `$`
- `location_escaped_close_brace.path` - location includes escaped `}` pattern
- `location_escaped_close_bracket.path` - location includes escaped `]` pattern
- `location_escaped_close_parenthesis.path` - location includes escaped `)` pattern
- `location_hash.path` - location includes `#`
- `location_open_brace.path` - location includes `{`
- `location_open_bracket.path` - location includes `[`
- `location_open_parenthesis.path` - location includes `(`
- `location_parentheses.path` - location includes `()`
- `location_pipe.path` - location includes `|`
- `location_redirect_greater.path` - location includes `>`
- `location_redirect_less.path` - location includes `<`
- `location_semicolon.path` - location includes `;`
- `location_square_brackets.path` - location includes `[]`
- `location_wildcard_question.path` - location includes `?`
- `location_wildcard_star.path` - location includes `*`
- `name_backtick.path` - name includes backtick
- `name_close_brace.path` - name includes `}`
- `name_close_bracket.path` - name includes `]`
- `name_close_parenthesis.path` - name includes `)`
- `name_empty_brackets.path` - name is `[]`
- `name_missing_closing_bracket.path` - name is missing closing `]`
- `name_missing_opening_bracket.path` - name is missing opening `[`
- `name_open_brace.path` - name includes `{`
- `name_open_bracket.path` - name includes `[`
- `name_open_parenthesis.path` - name includes `(`
- `options_backtick.path` - options include backtick
- `options_close_brace.path` - options include `}`
- `options_close_bracket.path` - options include `]`
- `options_close_parenthesis.path` - options include `)`
- `options_missing_closing_parenthesis.path` - options are missing closing `)`
- `options_missing_opening_parenthesis.path` - options are missing opening `(`
- `options_nested_braces.path` - options include nested braces
- `options_nested_brackets.path` - options include nested brackets
- `options_nested_parentheses.path` - options include nested parentheses
- `options_open_brace.path` - options include `{`
- `options_open_bracket.path` - options include `[`
- `options_open_parenthesis.path` - options include `(`

Maintenance note:
When adding a new file under `tests/paths/malicious/`, update both this section
and the `cases` list in `list_rejects_delimiter_malicious_cases` in
`tests/cli.rs` so fixture coverage and documentation stay aligned.

## Adding New Fixtures

1. Create a new `.path` file in this directory with a descriptive name
2. Use the `.path` file format:
   ```
   '<location>' [<name>] (<options>)
   ```
3. Update tests to use `copy_fixture_to_temp_store(dir, "fixture_name")` instead of inline `fs::write`
4. Update this README with the new fixture name and purpose

## Example Test Usage

```rust
#[test]
fn my_test() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    // Load the fixture instead of writing inline
    copy_fixture_to_temp_store(dir, "protected_entry").unwrap();

    // Now run test with the fixture data
    let mut cmd = test_cmd(dir, "");
    cmd.arg("verify");
    cmd.assert().success();
}
```
