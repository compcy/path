#!/usr/bin/env sh

# Source this file to make `path add`/`path remove` update PATH in your
# current shell automatically.
#
# Optional override:
#   PATH_CLI_BIN=/absolute/or/relative/path/to/path

path() {
    _path_cli_bin="${PATH_CLI_BIN:-}"
    if [ -z "$_path_cli_bin" ]; then
        _path_cli_bin="path"
    fi

    _path_stdout="$(command "$_path_cli_bin" "$@")"
    _path_status=$?

    if [ $_path_status -ne 0 ]; then
        if [ -n "$_path_stdout" ]; then
            printf '%s\n' "$_path_stdout"
        fi
        return $_path_status
    fi

    case "${1:-}" in
        add|remove|load)
            if [ -n "$_path_stdout" ]; then
                eval "$_path_stdout"
            fi
            ;;
        *)
            if [ -n "$_path_stdout" ]; then
                printf '%s\n' "$_path_stdout"
            fi
            ;;
    esac
}

path load
