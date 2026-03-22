# Path Wrapper Tests Documentation

This document explains what the path-wrapper.sh test files do in plain English, for readers not familiar with shell scripting.

## Overview

The wrapper script `path-wrapper.sh` is a shell-based security layer that safely manages PATH updates. It includes many utility functions that perform important operations. We have two test suites that verify these functions work correctly:

1. **Utility Function Tests** (`path-wrapper-functions.sh`) - Tests the helper functions
2. **Security Tests** (`path-wrapper-security.sh`) - Tests security hardening features

---

## Utility Function Tests (`tests/path-wrapper-functions.sh`)

This test file verifies that all the helper functions in the wrapper script work correctly with various inputs.

### String Transformation Tests

**`to_lower` function** - Converts text to lowercase

- `to_lower_uppercase`: Test that "ABCDEF" becomes "abcdef"
- `to_lower_mixed`: Test that "AbCdEf" becomes "abcdef"
- `to_lower_already_lowercase`: Test that lowercase text stays lowercase
- `to_lower_with_numbers`: Test that "ABC123DEF" becomes "abc123def"

**Why this matters**: The wrapper needs to normalize configuration values to lowercase for comparison. The tests ensure this conversion works correctly regardless of the input.

### Path Normalization Tests

**`normalize_policy_path` function** - Converts paths to their canonical (standard) form

- `normalize_absolute_dir`: Test that "/Users" stays "/Users"
- `normalize_trailing_slash`: Test that "/Users/" becomes "/Users" (removes the trailing slash)
- `normalize_empty_string_to_root`: Test that an empty string becomes "/" (the root directory)
- `normalize_root`: Test that "/" stays "/"
- `normalize_nonexistent_path`: Test that the function can handle paths that don't exist on the filesystem

**Why this matters**: Different systems might represent the same path in slightly different ways (with or without trailing slashes, through symlinks, etc.). The tests ensure that paths are converted to a standard format so comparisons work reliably.

### Shell Quote Decoding Tests

**`decode_single_quoted` function** - Safely extracts text from shell-escaped strings

In shell scripts, single quotes are used to prevent special character interpretation. Sometimes quotes need to be included in quoted strings, which requires special escaping (written as `'\''`).

- `decode_simple_quoted`: Test that "hello" correctly decodes to "hello"
- `decode_escaped_quote`: Test that "don't" (written as `don'\''t` in shell) decodes correctly
- `decode_unescaped_quote_fails`: Test that invalid quote escaping is rejected
- `decode_multiple_escaped_quotes`: Test that "x'y'z" (written as `x'\''y'\''z`) decodes correctly

**Why this matters**: The wrapper receives PATH values that are quoted and escaped by the shell. It needs to safely "unescape" these values to extract the actual path strings.

### PATH Value Validation Tests

**`validate_path_value` function** - Checks that PATH values are safe to use

PATH is a colon-separated list of directories. The validation function checks for common problems:

- `validate_simple_path`: Test that valid paths like "/usr/bin:/usr/local/bin" are accepted
- `validate_path_with_metacharacters`: Test that the function warns when PATH contains shell special characters (`;`, `&`, `|`, etc.)
- `validate_path_with_empty_segment`: Test that the function warns about leading colons (`:`, which create empty segments)
- `validate_path_double_colon`: Test that the function warns about `::` (double colon, which creates empty segments)

**Why this matters**: Malicious or malformed PATH values could break the shell environment or enable attacks. The validator checks for suspicious patterns and warns the user.

### Subcommand Extraction Tests

**`subcommand` function** - Identifies which subcommand the user is trying to run

Users run commands like `path add /some/dir` or `path list --pretty`. The function needs to extract the subcommand (`add`, `list`) from the arguments, while also handling command-line options like `--file=/path/to/store`.

- `subcommand_add`: Test that `path add /some/path` correctly identifies `add` as the subcommand
- `subcommand_with_options`: Test that `path --file /tmp/store.path add /some/path` correctly identifies `add` (not `--file`)
- `subcommand_with_equals_option`: Test that `path --file=/tmp/store.path remove` correctly identifies `remove`
- `subcommand_with_double_dash`: Test that `path -- load -flag` correctly identifies `load` (the `--` means "stop processing options")
- `subcommand_list` & `subcommand_load`: Test other subcommands are identified correctly

**Why this matters**: If the wrapper can't correctly identify which subcommand you want to run, it will run the wrong operation or fail entirely.

### SHA256 Checksum Tests

**`compute_sha256` function** - Calculates a cryptographic hash of the binary file

For security, the wrapper can verify that the path binary hasn't been tampered with by comparing its SHA-256 checksum. This is like a digital fingerprint that changes if the file is modified in any way.

- `compute_sha256_works`: Test that the function can calculate a valid 64-character hexadecimal hash
- `compute_sha256_deterministic`: Test that hashing the same file produces the same hash every time
- `compute_sha256_different_content`: Test that different files produce different hashes

**Why this matters**: If the wrapper relies on checksums to verify the binary is legitimate, the checksum calculation must be accurate and consistent.

### Export Application Tests

**`apply_export` function** - Safely applies PATH changes to the shell environment

When the path command finishes, it outputs a line like `export PATH='/new/path'`. The wrapper must safely parse and apply this update.

- `apply_export_valid_format`: Test that `export PATH='/new/path'` correctly updates PATH to `/new/path`
- `apply_export_with_empty_segments`: Test that the function applies paths even if they contain empty segments (with a warning)
- `apply_export_empty_output`: Test that the function rejects empty output (no update to apply)
- `apply_export_multiline_rejected`: Test that multi-line output is rejected (could be an injection attack)
- `apply_export_wrong_format`: Test that output without the "export" keyword is rejected
- `apply_export_quoted_colon`: Test that paths with colons like "/usr/bin:/usr/local/bin" work correctly

**Why this matters**: The wrapper must safely extract PATH values from shell commands without accidentally executing malicious code.

---

## Security Tests (`tests/path-wrapper-security.sh`)

This test file verifies that the wrapper correctly enforces security policies and prevents attacks.

### Test Setup

The security tests use fake `path` binaries to simulate various scenarios. The `write_fake_cli` helper creates a minimal fake binary that can be controlled to return different outputs.

### Individual Security Tests

**`test_relative_path_cli_bin_rejected`**

- Scenario: User sets `PATH_CLI_BIN="target/release/path"` (a relative path instead of absolute)
- Expected: The wrapper rejects this and shows a warning
- Why: Relative paths could be hijacked if the current directory changes

**`test_allowlist_rejects_untrusted_binary`**

- Scenario: User sets `PATH_CLI_ALLOWLIST="/definitely/not/trusted"` but the path binary is somewhere else
- Expected: The wrapper rejects the binary and shows a warning
- Why: The allowlist is a whitelist of trusted locations; anything outside is rejected

**`test_allowlist_accepts_trusted_binary`**

- Scenario: User sets `PATH_CLI_ALLOWLIST="/tmp"` and the binary is in `/tmp`
- Expected: The wrapper accepts the binary
- Why: Verifies that whitelisting actually works when the binary is in an approved location

**`test_checksum_rejects_mismatch`**

- Scenario: User sets `PATH_CLI_SHA256` to an expected checksum, but the binary has a different checksum
- Expected: The wrapper rejects the binary and shows a warning
- Why: If the binary's checksum doesn't match, it may have been tampered with

**`test_checksum_accepts_match`**

- Scenario: User sets `PATH_CLI_SHA256` to the actual checksum of the binary
- Expected: The wrapper accepts the binary
- Why: Verifies that the checksum verification actually works

**`test_injection_output_is_not_executed`**

- Scenario: The fake binary outputs something like `export PATH='/usr/bin'; touch /tmp/pwned`
- Expected: The touch command is NOT executed (the injection attack fails)
- Why: This is a critical security test. The wrapper must never execute shell commands from the path binary's output. It should only apply the PATH value, not run arbitrary commands.

**`test_suspicious_payload_warns_without_eval`**

- Scenario: The fake binary outputs a PATH value that contains shell metacharacters like `;echo suspicious`
- Expected: The wrapper warns but still applies the PATH (without executing the echo)
- Why: If user configuration allows it, suspicious payloads should be warned about but not executed

---

## How to Run the Tests

```sh
# Run the utility function tests
sh tests/path-wrapper-functions.sh

# Run the security tests
sh tests/path-wrapper-security.sh

# Run both
sh tests/path-wrapper-functions.sh && sh tests/path-wrapper-security.sh
```

## What the Tests Verify

Together, these tests verify:

1. **Correctness**: All utility functions handle normal inputs correctly
2. **Edge Cases**: Functions work with unusual but valid inputs (empty strings, nonexistent paths, etc.)
3. **Security**: The wrapper prevents known attack vectors (injection, binary swapping, tampering)
4. **Configuration**: Security features like allowlists and checksums work as intended
5. **Parsing**: Shell output is correctly parsed and applied without executing injected code

If any test fails, a message like "FAIL: describe what went wrong" is printed, and the test suite stops.

---

## Common Test Patterns

Most tests follow this pattern:

1. **Setup** - Create temporary files or set up test conditions
2. **Execute** - Run the function being tested with specific inputs
3. **Verify** - Check that the output is what we expected
4. **Report** - Print PASS or FAIL with a description

For example:

```sh
test_to_lower_uppercase() {
    # Execute the function
    result=$(_path_wrapper_to_lower "ABCDEF")

    # Verify the result
    [ "$result" = "abcdef" ] || fail "test failed"

    # Report success
    pass "to_lower handles uppercase"
}
```
