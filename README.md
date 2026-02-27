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
- `path add <location> [name]` — append a new entry to the PATH; name is
  optional and, if omitted, defaults to the location string
- `path add --pre <location> [name]` — prepend instead of append
- `path add --exclusive …` — mark entry exclusive (extra field stored)

Example:

```sh
path add /usr/local/bin             # store entry with name "/usr/local/bin"
path add /home/$USER/.bin home      # store with short name "home"
path add --pre /opt/custom/bin      # prepend to PATH instead of append
```

Entries are persisted to a `.path` file in the current directory. Each line
consists of `location<TAB>name<TAB>exclusivity?`.  The tool reads and writes
this file automatically when adding.

You can also install a release build and invoke it directly:

```sh
cargo install --path .
path add /some/dir
```

Additional functionality (removing, restoring, etc.) may appear in future
releases.

