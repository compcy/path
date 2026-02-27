# path

`path` is a simple command-line utility for inspecting and manipulating the
`PATH` environment variable. Its initial implementation prints the current
value of `PATH` to standard output and provides subcommands for adding
entries.

## Usage

```sh
# build and run with Cargo
cargo run -- [OPTIONS] [SUBCOMMAND]
```

### Commands

- `path` — display current PATH
- `path add <location>` — append a new entry to the PATH
- `path add --pre <location>` — prepend instead of append

Example:

```sh
path add /usr/local/bin             # append
path add --pre /opt/custom/bin      # prepend
```

You can also install a release build and invoke it directly:

```sh
cargo install --path .
path add /some/dir
```

Additional functionality (removing, restoring, persistence, etc.) will be
definitions added in future releases.

