#!/usr/bin/env sh

# Source this file to make `path add`/`path remove` update PATH in your
# current shell automatically.
#
# Optional override:
#   PATH_CLI_BIN=/absolute/path/to/path
# Optional hardening:
#   PATH_CLI_ALLOWLIST=/opt/homebrew/bin:/usr/local/bin/path
#   PATH_CLI_SHA256=<expected-hex-sha256>

# Security notes:
# - The wrapper resolves and locks an absolute `path` binary on first use.
# - For `add`, `remove`, and `load`, command output is parsed and applied
#   without directly eval'ing arbitrary command text.
# - Suspicious configuration or output triggers clear security warnings.

_PATH_WRAPPER_LOCKED_BIN=""
_PATH_WRAPPER_CR="$(printf '\r')"
_PATH_WRAPPER_ACTIVE_BIN=""

_path_wrapper_warn() {
    printf '%s\n' "PATH WRAPPER SECURITY WARNING: $*" >&2
}

_path_wrapper_to_lower() {
    printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

_path_wrapper_compute_sha256() {
    _path_hash_file=$1

    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$_path_hash_file" | awk '{print $1}'
        return 0
    fi

    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$_path_hash_file" | awk '{print $1}'
        return 0
    fi

    if command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$_path_hash_file" | sed 's/^.*= //'
        return 0
    fi

    _path_wrapper_warn "unable to verify checksum: no SHA-256 tool found (tried shasum, sha256sum, openssl)."
    return 1
}

_path_wrapper_normalize_policy_path() {
    _path_policy=$1
    _path_policy=${_path_policy%/}

    if [ -z "$_path_policy" ]; then
        _path_policy="/"
    fi

    if [ -d "$_path_policy" ]; then
        _path_policy_real="$(cd -P "$_path_policy" 2>/dev/null && pwd)"
        if [ -n "$_path_policy_real" ]; then
            printf '%s\n' "$_path_policy_real"
            return 0
        fi
    fi

    if [ -e "$_path_policy" ]; then
        _path_policy_parent=${_path_policy%/*}
        _path_policy_base=${_path_policy##*/}
        if [ -z "$_path_policy_parent" ]; then
            _path_policy_parent="/"
        fi

        _path_policy_parent_real="$(cd -P "$_path_policy_parent" 2>/dev/null && pwd)"
        if [ -n "$_path_policy_parent_real" ]; then
            printf '%s/%s\n' "$_path_policy_parent_real" "$_path_policy_base"
            return 0
        fi
    fi

    printf '%s\n' "$_path_policy"
}

_path_wrapper_enforce_allowlist() {
    _path_resolved=$1
    _path_allowlist=${PATH_CLI_ALLOWLIST:-}

    if [ -z "$_path_allowlist" ]; then
        return 0
    fi

    _path_remaining=$_path_allowlist
    _path_has_usable_entry=0

    while :; do
        case "$_path_remaining" in
            *:*)
                _path_allow_entry=${_path_remaining%%:*}
                _path_remaining=${_path_remaining#*:}
                ;;
            *)
                _path_allow_entry=$_path_remaining
                _path_remaining=""
                ;;
        esac

        if [ -z "$_path_allow_entry" ]; then
            _path_wrapper_warn "PATH_CLI_ALLOWLIST contains an empty entry."
        else
            case "$_path_allow_entry" in
                /*)
                    _path_has_usable_entry=1
                    _path_allow_normalized="$(_path_wrapper_normalize_policy_path "$_path_allow_entry")"

                    if [ "$_path_allow_normalized" = "/" ]; then
                        return 0
                    fi

                    if [ "$_path_resolved" = "$_path_allow_normalized" ]; then
                        return 0
                    fi

                    case "$_path_resolved" in
                        "$_path_allow_normalized"/*)
                            return 0
                            ;;
                    esac
                    ;;
                *)
                    _path_wrapper_warn "ignoring non-absolute PATH_CLI_ALLOWLIST entry '$_path_allow_entry'."
                    ;;
            esac
        fi

        if [ -z "$_path_remaining" ]; then
            break
        fi
    done

    if [ "$_path_has_usable_entry" -eq 0 ]; then
        _path_wrapper_warn "PATH_CLI_ALLOWLIST is set but has no usable absolute entries."
    fi

    _path_wrapper_warn "binary '$_path_resolved' is not permitted by PATH_CLI_ALLOWLIST."
    return 1
}

_path_wrapper_verify_checksum() {
    _path_resolved=$1
    _path_expected=${PATH_CLI_SHA256:-}

    if [ -z "$_path_expected" ]; then
        return 0
    fi

    _path_expected="$(_path_wrapper_to_lower "$_path_expected")"
    case "$_path_expected" in
        *[!0-9a-f]*)
            _path_wrapper_warn "PATH_CLI_SHA256 must be lowercase/uppercase hex; refusing provided value."
            return 1
            ;;
    esac

    if [ "${#_path_expected}" -ne 64 ]; then
        _path_wrapper_warn "PATH_CLI_SHA256 must be exactly 64 hex characters."
        return 1
    fi

    _path_actual="$(_path_wrapper_compute_sha256 "$_path_resolved")" || return 1
    _path_actual="$(_path_wrapper_to_lower "$_path_actual")"

    if [ "$_path_actual" != "$_path_expected" ]; then
        _path_wrapper_warn "checksum mismatch for '$_path_resolved' (expected $_path_expected, got $_path_actual)."
        return 1
    fi

    return 0
}

_path_wrapper_resolve_cli_bin() {
    if [ -n "$_PATH_WRAPPER_LOCKED_BIN" ]; then
        _PATH_WRAPPER_ACTIVE_BIN="$_PATH_WRAPPER_LOCKED_BIN"
        return 0
    fi

    _path_candidate="${PATH_CLI_BIN:-}"
    if [ -n "$_path_candidate" ]; then
        case "$_path_candidate" in
            /*) ;;
            *)
                _path_wrapper_warn "PATH_CLI_BIN must be absolute; refusing '$_path_candidate'."
                return 1
                ;;
        esac
    else
        _path_candidate="$(command -v path 2>/dev/null || true)"
        if [ -z "$_path_candidate" ]; then
            _path_wrapper_warn "unable to locate 'path'; set PATH_CLI_BIN to an absolute path."
            return 1
        fi

        case "$_path_candidate" in
            /*) ;;
            *)
                _path_wrapper_warn "resolved non-absolute command '$_path_candidate'; set PATH_CLI_BIN to an absolute path."
                return 1
                ;;
        esac
    fi

    if [ ! -x "$_path_candidate" ]; then
        _path_wrapper_warn "resolved binary '$_path_candidate' is not executable."
        return 1
    fi

    _path_dir=${_path_candidate%/*}
    _path_base=${_path_candidate##*/}
    if [ -z "$_path_dir" ]; then
        _path_dir="/"
    fi

    _path_real_dir="$(cd -P "$_path_dir" 2>/dev/null && pwd)"
    if [ -z "$_path_real_dir" ]; then
        _path_wrapper_warn "unable to canonicalize directory for '$_path_candidate'."
        return 1
    fi

    _path_resolved="${_path_real_dir}/${_path_base}"
    if [ ! -x "$_path_resolved" ]; then
        _path_wrapper_warn "resolved absolute binary '$_path_resolved' is not executable."
        return 1
    fi

    if [ -L "$_path_candidate" ]; then
        _path_wrapper_warn "binary path '$_path_candidate' is a symlink; verify ownership and target."
    fi

    if [ -w "$_path_resolved" ]; then
        _path_wrapper_warn "binary '$_path_resolved' is writable by the current user; verify trust."
    fi

    case "$_path_resolved" in
        *[[:space:]]*)
            _path_wrapper_warn "binary path '$_path_resolved' contains whitespace."
            ;;
    esac

    _path_wrapper_enforce_allowlist "$_path_resolved" || return 1
    _path_wrapper_verify_checksum "$_path_resolved" || return 1

    _PATH_WRAPPER_LOCKED_BIN="$_path_resolved"
    _PATH_WRAPPER_ACTIVE_BIN="$_PATH_WRAPPER_LOCKED_BIN"
}

_path_wrapper_subcommand() {
    _path_expect_value=0

    for _path_arg in "$@"; do
        if [ "$_path_expect_value" -eq 1 ]; then
            _path_expect_value=0
            continue
        fi

        case "$_path_arg" in
            --file)
                _path_expect_value=1
                ;;
            --file=*)
                ;;
            --)
                # End of options; remaining values are positional arguments.
                _path_expect_value=2
                ;;
            -*)
                ;;
            *)
                printf '%s\n' "$_path_arg"
                return 0
                ;;
        esac

        if [ "$_path_expect_value" -eq 2 ]; then
            _path_expect_value=0
        fi
    done

    return 1
}

_path_wrapper_decode_single_quoted() {
    _path_encoded=$1
    _path_decoded=""

    while :; do
        case "$_path_encoded" in
            *"'\\''"*)
                _path_prefix=${_path_encoded%%"'\\''"*}
                _path_decoded=${_path_decoded}${_path_prefix}"'"
                _path_encoded=${_path_encoded#*"'\\''"}
                ;;
            *"'"*)
                return 1
                ;;
            *)
                _path_decoded=${_path_decoded}${_path_encoded}
                printf '%s' "$_path_decoded"
                return 0
                ;;
        esac
    done
}

_path_wrapper_validate_path_value() {
    _path_value=$1

    case "$_path_value" in
        *'
'*)
            _path_wrapper_warn "refusing PATH update with embedded newlines."
            return 1
            ;;
    esac

    case "$_path_value" in
        *"$_PATH_WRAPPER_CR"*)
            _path_wrapper_warn "refusing PATH update with carriage returns."
            return 1
            ;;
    esac

    case "$_path_value" in
        *';'*|*'&'*|*'|'*|*'$'*|*'<'*|*'>'*|*'('*|*')'*)
            _path_wrapper_warn "PATH value contains shell metacharacters; applying without eval."
            ;;
    esac

    case "$_path_value" in
        :*|*::|*:)
            _path_wrapper_warn "PATH contains empty segments (leading/trailing ':' or '::')."
            ;;
    esac
}

_path_wrapper_apply_export() {
    _path_export_line=$1

    if [ -z "$_path_export_line" ]; then
        _path_wrapper_warn "received empty output for PATH-changing command; refusing to apply."
        return 1
    fi

    case "$_path_export_line" in
        *'
'*)
            _path_wrapper_warn "received multiline command output for PATH update; refusing to apply."
            return 1
            ;;
    esac

    case "$_path_export_line" in
        export\ PATH=\'*\')
            ;;
        *)
            _path_wrapper_warn "unexpected PATH update format; refusing to apply output."
            printf '%s\n' "$_path_export_line" >&2
            return 1
            ;;
    esac

    _path_payload=${_path_export_line#export PATH=\'}
    _path_payload=${_path_payload%\'}

    _path_decoded="$(_path_wrapper_decode_single_quoted "$_path_payload")" || {
        _path_wrapper_warn "failed to parse shell-escaped PATH value; refusing to apply."
        return 1
    }

    _path_wrapper_validate_path_value "$_path_decoded" || return 1

    PATH=$_path_decoded
    export PATH
}

path() {
    _path_wrapper_resolve_cli_bin || return 1
    _path_cli_bin="$_PATH_WRAPPER_ACTIVE_BIN"

    _path_stdout="$(command "$_path_cli_bin" "$@")"
    _path_status=$?

    if [ $_path_status -ne 0 ]; then
        if [ -n "$_path_stdout" ]; then
            printf '%s\n' "$_path_stdout"
        fi
        return $_path_status
    fi

    _path_subcommand="$(_path_wrapper_subcommand "$@")"

    case "$_path_subcommand" in
        add|remove|load)
            if [ -n "$_path_stdout" ]; then
                _path_wrapper_apply_export "$_path_stdout" || return 1
            else
                _path_wrapper_warn "no output from PATH-changing command; refusing to continue."
                return 1
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
