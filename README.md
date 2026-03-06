# path

`path` is a simple command-line utility for inspecting and manipulating the
`PATH` environment variable. It also keeps a local record of added entries in
a plain-text `.path` file, with optional names and autoset flags.

Because a child process cannot directly modify its parent shell environment,
commands that compute PATH output a shell assignment like
`export PATH='...new value...'`.

If you prefer not to type `eval` each time, source the wrapper script once:

```sh
. ./path-wrapper.sh
```

Then `path add ...` and `path remove ...` automatically apply to the current
shell PATH.

## Usage

```sh
# build and run with Cargo
cargo run -- [OPTIONS] [SUBCOMMAND]

# apply the new PATH in your current shell
eval "$(path add /some/dir mydir)"

# or source the wrapper once and run directly
. ./path-wrapper.sh
path add /some/dir mydir
```

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
  - Only entries with an explicit `name` are written to `.path`.
  - Existing PATH entries are not duplicated for equivalent trailing-slash forms (for example `/usr/local/bin` and `/usr/local/bin/`).
- `path add --pre <location-or-name> [name]` — prepend instead of append
- `path remove <location-or-name>` — remove from PATH only
  - If the argument matches a stored short name, its location is removed from PATH.
  - Otherwise the argument is treated as a path (same absolute/dot-relative validation, and no `:`).
  - This command does not modify `.path`.
- `path delete <location-or-name>` — delete from `.path` only
  - If the argument matches a stored short name, that entry is deleted.
  - Otherwise the argument is treated as a path (same absolute/dot-relative validation, and no `:`), and matching stored locations are deleted.
- `path list` — show all saved entries from the `.path` file
- `path load` — append all stored entries marked `auto` to PATH

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
path load                            # add only entries marked auto

# invalid unless "foo" is a stored name
path add foo

# invalid because .path is a file, not a directory
path add .path
```

Entries are persisted to a `.path` file in the current directory, but
only for entries where you supplied an explicit name. New lines are written as
`location name autoset?` with fields separated by whitespace, where autoset is
`auto` or `noauto`.
If the third field is missing, it is treated as `auto`. (Because a name is
mandatory the tool will refuse to start if it finds a line missing that
field.) A trailing `/` on stored paths is normalized away while reading
(except for `/`). The tool reads and writes this file
automatically when adding.

When a stored location contains whitespace (or `\`), it is escaped with `\`
so the file remains whitespace-delimited. For example:

```text
/opt/my\ tools tools auto
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

