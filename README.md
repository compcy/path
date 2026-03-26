# path

`path` is a command-line utility for inspecting and managing your shell `PATH`.
It tracks named path entries in a plain-text store file (default `~/.path`) and
can automatically apply them on each new terminal session.

Because a child process cannot modify its parent shell environment,
PATH-mutating commands print a shell assignment (`export PATH='...'`).
Use the companion wrapper script (see [Shell Integration](#shell-integration))
so the shell function applies those changes automatically.

## Sample Output

Running `path` with no subcommand shows your current PATH as a formatted table:

```
#   PATH                     NAME          TYPE
--  -----------------------  ------------  ------------------
1   /opt/homebrew/bin        homebrewbin   known
2   /usr/local/bin           usrlocalbin   system [protected]
3   /usr/bin                 usrbin        system [protected]
4   /bin                     sysbin        system [protected]
5   /usr/sbin                usrsbin       system [protected]
6   /sbin                    syssbin       system [protected]
7   /home/alice/.cargo/bin   cargo
```

- **#** — 1-based position in PATH
- **NAME** — resolved from the store file, or from the built-in known/system path list; blank when unknown
- **TYPE** — `system` for protected system paths, `known` for recognized extras; `[protected]` appended when removal is blocked

Column widths adjust to the widest value in each column. If the store file is
missing, output still succeeds and prints a warning to stderr.

## Installation

### Install with Cargo

```sh
cargo install --path .
```

### Build and run directly

```sh
cargo build
./target/debug/path
```

### Shell Integration

To have PATH mutations take effect in your current shell automatically, source
the wrapper script from your rc file:

```sh
# ~/.zshrc or ~/.bashrc
. "$HOME/git/path/path-wrapper.sh"
```

Sourcing the wrapper defines a `path` shell function and immediately runs
`path load`, applying your stored auto entries on each new terminal session.

If `path` is not yet on PATH when your rc file runs, set `PATH_CLI_BIN` first:

```sh
PATH_CLI_BIN="$HOME/git/path/target/debug/path"
. "$HOME/git/path/path-wrapper.sh"
```

For stricter hardening, you can pin trusted install locations and checksum the binary:

```sh
PATH_CLI_BIN="/opt/homebrew/bin/path"
PATH_CLI_ALLOWLIST="/opt/homebrew/bin:/usr/local/bin/path"
PATH_CLI_SHA256="$(shasum -a 256 "$PATH_CLI_BIN" | awk '{print $1}')"
. "$HOME/git/path/path-wrapper.sh"
```

`PATH_CLI_ALLOWLIST` is a colon-delimited list of absolute paths; each entry
can be an exact binary path or a directory prefix. `PATH_CLI_SHA256` must be
the 64-character hex SHA-256 digest — if it does not match, the wrapper refuses
to run.

## Simple Usage

These commands affect only the current shell session and do not persist anything
to the store file.

### View PATH

```sh
path
```

### Add to PATH

```sh
# Append a directory to PATH
path add /usr/local/mytools

# Prepend a directory to PATH
path add --pre /usr/local/mytools

# Resolve and append the current directory
path add .
```

Arguments must be an absolute path (`/…`) or dot-relative (`./…`, `../…`).
Trailing `/` is stripped, relative paths are canonicalized, and the path must
be a directory if it already exists. Paths must not contain `:`.

### Remove from PATH

```sh
# Remove by path
path remove /usr/local/mytools

# Force-remove a protected path
path remove --force /bin
path remove -f /bin
```

`path remove` does not modify the store file.

## Stored Entries

Named entries are persisted to `~/.path` and reloaded automatically in future
shells (via `path load`). Only entries with an explicit name are written to the
store file.

### Add a Named Entry

```sh
# Append and store with a short name
path add /home/$USER/.cargo/bin cargo

# Prepend and store (load will also prepend in future shells)
path add --pre /opt/custom/bin custom

# Store but exclude from automatic loading
path add /opt/tools/bin tools --noauto

# Store and prevent removal by path remove
path add /opt/locked/bin locked --protect
```

Names must be alphanumeric and unique. The built-in system-path names
(`sysbin`, `syssbin`, `usrbin`, `usrsbin`, `usrlocalbin`, `usrlocalsbin`) are
reserved and cannot be used for stored entries.

You can also use a stored name as shorthand for its location:

```sh
path add cargo    # equivalent to: path add /home/$USER/.cargo/bin
```

### List Stored Entries

```sh
path list
```

Displays stored entries in a tabular format with aligned columns:

```
PATH               NAME     OPTIONS
-----------------  -------  -------
/usr/local/bin     cargo    auto
/opt/homebrew/bin  brew     auto,pre
/opt/tools         tools    noauto
```

### Delete a Stored Entry

Removes the entry from `~/.path` but does not change the current PATH:

```sh
path delete cargo                    # by name
path delete /home/alice/.cargo/bin   # by path
```

### Load Stored Entries

Adds all `auto` entries from `~/.path` to the current PATH. Entries with `pre`
are prepended; all others are appended.

```sh
path load
```

This runs automatically at shell startup when the wrapper is sourced.
Unknown option tokens in the store file produce a warning but do not stop loading.

### Verify the Store File

Validates `~/.path` and reports any errors:

```sh
path verify
```

Prints `Path file is valid.` on success, or error details and exits non-zero on
failure. Unknown option tokens that are only warnings during `path load` are
**fatal** under `path verify`.

## Manual File Editing

You can edit `~/.path` directly in any text editor. The format is:

```text
# layout: '<location>' [<name>] (<options>)
'/home/alice/.cargo/bin' [cargo] (auto)
'/opt/custom/bin' [custom] (auto,pre)
'/opt/locked/bin' [locked] (auto,protect)
'/opt/tools/bin' [tools] (noauto)
```

Valid options are `auto`, `noauto`, `pre`, and `protect`. A missing options
field defaults to `auto`. Locations are enclosed in single quotes; literal `'`
and `\` are escaped as `\'` and `\\`.

> **After editing, always run `path verify` to catch mistakes before they affect your shell.**

## Using a Specific Store File

The global `--file` option points any command at an alternate store file
instead of the default `~/.path`:

```sh
path --file /path/to/myfile.path list
path --file /path/to/myfile.path load
path --file /path/to/myfile.path verify
```

This is useful for managing multiple PATH profiles or testing changes before
applying them. Note: `-f` is **not** a global alias for `--file`; it is only
valid as `path remove -f`.

## Restoring System Paths

`path restore` adds the built-in set of protected system paths to the current
PATH without storing them:

```sh
path restore
```

Restored paths (in order): `/bin`, `/sbin`, `/usr/bin`, `/usr/sbin`,
`/usr/local/bin`, `/usr/local/sbin`.

## Store Validation Rules

Commands that read stored entries (`add`, `remove`, `delete`, `list`, `load`,
`verify`) abort if they encounter:

- a nameless entry
- a relative or non-canonical stored location
- a stored location containing `:`
- a non-alphanumeric name
- duplicate names

The default `path` table view is intentionally tolerant — it still prints the
PATH table and emits store warnings/errors to stderr afterward. Missing
filesystem locations produce warnings only and are never auto-removed.

Any content read from the store file that is reflected in error or warning
messages is sanitized before display: control characters (such as ANSI escape
sequences) and invisible Unicode code points (bidirectional overrides,
zero-width characters, BOM, etc.) are replaced with their `\u{XXXX}` escape
form. This prevents a crafted store file from injecting terminal control
sequences via diagnostic output.

## Command Reference

| Command                       | Description                                   |
| ----------------------------- | --------------------------------------------- |
| `path`                        | Show current PATH as a formatted table        |
| `path add <dir> [name]`       | Append to PATH; store if name given           |
| `path add --pre <dir> [name]` | Prepend to PATH; store if name given          |
| `path remove <dir-or-name>`   | Remove from current PATH only                 |
| `path delete <dir-or-name>`   | Delete from store file only                   |
| `path list`                   | Show all stored entries                       |
| `path load`                   | Apply all auto entries from the store to PATH |
| `path verify`                 | Validate store file                           |
| `path restore`                | Restore built-in system paths to PATH         |

## CI Checks

The CI workflow builds, tests, and validates documentation coverage. To run
checks locally:

```sh
# Documentation coverage check
RUSTDOCFLAGS="-D warnings -W missing-docs" cargo doc --no-deps --document-private-items

# Wrapper security regression tests
sh tests/path-wrapper-security.sh

# Wrapper utility function tests
sh tests/path-wrapper-functions.sh
```

## License

This project is licensed under the MIT License. See `LICENSE`.

All Rust dependencies are required to have an SPDX license expression that
includes `MIT`. CI enforces this policy.
