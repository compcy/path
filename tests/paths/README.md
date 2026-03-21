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
- `legacy_unwrapped.path` - Legacy format without wrapping delimiters (error case)
- `one_invalid_one_valid.path` - Mix of valid and invalid locations (warning case)

## Adding New Fixtures

1. Create a new `.path` file in this directory with a descriptive name
2. Use the `.path` file format:
   ```
   '<location>' [<name>] (<options>)
   ```
3. Update tests to use `write_fixture_to_store(dir, "fixture_name")` instead of inline `fs::write`
4. Update this README with the new fixture name and purpose

## Example Test Usage

```rust
#[test]
fn my_test() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    // Load the fixture instead of writing inline
    write_fixture_to_store(dir, "protected_entry").unwrap();

    // Now run test with the fixture data
    let mut cmd = test_cmd(dir, "");
    cmd.arg("verify");
    cmd.assert().success();
}
```
