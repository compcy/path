# path

`path` is a simple command-line utility for inspecting and manipulating the
`PATH` environment variable. It also keeps a local record of added entries in
a plain-text store file (default: `$HOME/.path`), with optional names and
autoset flags.

Because a child process cannot directly modify its parent shell environment,
commands that compute PATH output a shell assignment like
`export PATH='...new value...'`.

For a persistent setup, source the wrapper script from your shell rc file
(`~/.zshrc`, `~/.bashrc`, etc.). 

```sh
# ~/.zshrc (or ~/.bashrc)
. "$HOME/git/path/path-wrapper.sh"
```

Sourcing `path-wrapper.sh` defines the shell function and immediately runs
`path load`, so each new terminal session loads auto entries from `.path`.

If `path` is not yet on PATH when your rc file runs, set `PATH_CLI_BIN` before
sourcing the wrapper:

```sh
PATH_CLI_BIN="$HOME/git/path/target/debug/path"
. "$HOME/git/path/path-wrapper.sh"
```

For stricter wrapper hardening, you can optionally pin trusted install
locations and the binary checksum:

```sh
PATH_CLI_BIN="/opt/homebrew/bin/path"
PATH_CLI_ALLOWLIST="/opt/homebrew/bin:/usr/local/bin/path"
PATH_CLI_SHA256="$(shasum -a 256 "$PATH_CLI_BIN" | awk '{print $1}')"
. "$HOME/git/path/path-wrapper.sh"
```

`PATH_CLI_ALLOWLIST` is a colon-delimited list of absolute paths. Each entry
can be an exact binary path (for example `/usr/local/bin/path`) or an absolute
directory prefix (for example `/opt/homebrew/bin`).

`PATH_CLI_SHA256`, when set, must be the exact 64-character hex SHA-256 digest
for the resolved binary path. If it does not match, the wrapper refuses to run.

Move any or your paths from your rc file into .path:
export PATH="$HOME/.cargo/bin:$PATH"

```sh
path add $HOME/.cargo/bin cargo
```

## Usage

```sh
# build and run with Cargo
cargo run -- [--file <path>] [SUBCOMMAND]

# one-off usage without the wrapper
eval "$(path add /some/dir mydir)"

# after rc-file setup, this updates PATH directly
path add /some/dir mydir
```

Global option:

- `--file <path>` — use a specific store file instead of the default `$HOME/.path`.
- `-f` is intentionally not available (reserved for a future `--force` option).

### Commands

- `path` — display current PATH
- `path add <location-or-name> [name]` — append to PATH.
  - If `<location-or-name>` matches a stored short name, that stored location is used.
  - Otherwise it must be an absolute path (`/…`) or dot-relative (`./…`, `../…`).
  - Quote path arguments that contain spaces (for example `"./my tools"`).
  - Path arguments must not contain `:`.
  - Relative path arguments are canonicalized once (for example `.` becomes the absolute current directory).
  - Absolute path arguments are used as provided.
  - Trailing `/` is stripped from path arguments (except `/` itself).
  - If the path exists, it must be a directory (files are rejected).
  - If `name` is provided, it must be alphanumeric and unique.
  - Use `--noauto` to store a named entry that should not be included by `path load`.
  - Only entries with an explicit `name` are written to the configured store file.
  - Existing PATH entries are not duplicated for equivalent trailing-slash forms (for example `/usr/local/bin` and `/usr/local/bin/`).
- `path add --pre <location-or-name> [name]` — prepend instead of append
- `path remove <location-or-name>` — remove from PATH only
  - If the argument matches a stored short name, its location is removed from PATH.
  - Otherwise the argument is treated as a path (same absolute/dot-relative validation, and no `:`).
  - This command does not modify `.path`.
- `path delete <location-or-name>` — delete from the configured store file only
  - If the argument matches a stored short name, that entry is deleted.
  - Otherwise the argument is treated as a path (same absolute/dot-relative validation, and no `:`), and matching stored locations are deleted.
- `path list` — show all saved entries from the configured store file
- `path load` — append all stored entries marked `auto` to PATH
  - This runs automatically when `path-wrapper.sh` is sourced (for example at shell startup from your rc file).
- `path verify` — validate configured store entries and print `Path file is valid.` when validation passes
  - If the configured store file does not exist or has no entries, it fails.
  - On validation failure, it prints the failure details and exits non-zero.

**Startup validation note:** when reading `.path`, the tool aborts if it finds:
- a nameless entry,
- a relative or non-canonical-looking stored location,
- a stored location containing `:`,
- a non-alphanumeric name,
- duplicate names.

Missing filesystem locations only produce warnings (they are not auto-removed).

Example:

```sh
path add /usr/local/bin             # append only; not stored (no explicit name)
path add /home/$USER/.bin home      # store with short name "home"
path add "./my tools" mytools       # path contains a space
path add /opt/internal/bin internal --noauto  # store but do not include in `path load`
path add --pre /opt/custom/bin      # prepend to PATH instead of append
path add home                        # uses stored name "home" if present
path remove /home/$USER/.bin         # remove by path
path remove home                     # remove from PATH by stored short name
path delete home                     # delete stored entry from .path by name
path load                            # add only entries marked auto (usually automatic at shell startup)
path verify                          # validate .path contents and report status

# invalid unless "foo" is a stored name
path add foo

# invalid because .path is a file, not a directory
path add .path
```

Entries are persisted to `$HOME/.path` by default (or the file passed with
`--file`), but only for entries where you supplied an explicit name. New lines are written as
`location [name] (options)` with fields separated by whitespace, where options
currently use `auto` or `noauto`.
If the third field is missing, it is treated as `auto`. (The name field is
mandatory, and the tool refuses to start if it finds a line without a valid
name.) A trailing `/` on stored paths is normalized away while reading
(except for `/`). The tool reads and writes this file
automatically when adding.

When a stored location contains whitespace (or `\`), it is escaped with `\`
so the file remains whitespace-delimited. For example:

```text
/opt/my\ tools [tools] (auto)
```

You can also install a release build and invoke it directly:

```sh
cargo install --path .
path add /some/dir
```

## CI checks

The CI workflow builds, tests, and validates documentation coverage for private
and public items. To run the docs check locally:

```sh
RUSTDOCFLAGS="-D warnings -W missing-docs" cargo doc --no-deps --document-private-items
```

To run wrapper security regression tests locally:

```sh
sh tests/path-wrapper-security.sh
```

## License

This project is licensed under the MIT License. See `LICENSE`.

All Rust dependencies are required to have an SPDX license expression that
includes `MIT`. CI enforces this policy.

