# path

`path` is a simple command-line utility for inspecting and manipulating the
`PATH` environment variable. It also keeps a local record of added entries in
a plain-text `.path` file, with optional names and exclusivity flags.

## Usage

```sh
# build and run with Cargo
cargo run -- [OPTIONS] [SUBCOMMAND]
```

### Commands

- `path` — display current PATH
- `path add <location-or-name> [name]` — append to PATH.
  - If `<location-or-name>` matches a stored short name, that stored location is used.
  - Otherwise it must be an absolute path (`/…`) or dot-relative (`./…`, `../…`).
  - If the path exists, it must be a directory (files are rejected).
  - If `name` is provided, it must be alphanumeric and unique.
  - Only entries with an explicit `name` are written to `.path`.
- `path add --pre <location-or-name> [name]` — prepend instead of append
- `path remove <location-or-name>` — remove from PATH only
  - If the argument matches a stored short name, its location is removed from PATH.
  - Otherwise the argument is treated as a path (same absolute/dot-relative validation).
  - This command does not modify `.path`.
- `path delete <location-or-name>` — delete from `.path` only
  - If the argument matches a stored short name, that entry is deleted.
  - Otherwise the argument is treated as a path (same absolute/dot-relative validation), and matching stored locations are deleted.
- `path list` — show all saved entries from the `.path` file

**Startup validation note:** when reading `.path`, the tool aborts if it finds:
- a nameless entry,
- a non-alphanumeric name,
- duplicate names.

Missing filesystem locations only produce warnings (they are not auto-removed).

Example:

```sh
path add /usr/local/bin             # append only; not stored (no explicit name)
path add /home/$USER/.bin home      # store with short name "home"
path add --pre /opt/custom/bin      # prepend to PATH instead of append
path add home                        # uses stored name "home" if present
path remove /home/$USER/.bin         # remove by path
path remove home                     # remove from PATH by stored short name
path delete home                     # delete stored entry from .path by name

# invalid unless "foo" is a stored name
path add foo

# invalid because .path is a file, not a directory
path add .path
```

Entries are persisted to a `.path` file in the current directory, but
only for entries where you supplied an explicit name. Each line consists of
`location<TAB>name<TAB>exclusivity?`. (Because a name is mandatory the tool
will refuse to start if it finds a line missing that field.) The tool reads and writes this file
automatically when adding.

You can also install a release build and invoke it directly:

```sh
cargo install --path .
path add /some/dir
```

