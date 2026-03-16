#!/usr/bin/env sh
set -eu

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

fail() {
    printf '%s\n' "FAIL: $*" >&2
    exit 1
}

pass() {
    printf '%s\n' "PASS: $*"
}

sha256_file() {
    _target_file=$1

    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$_target_file" | awk '{print $1}'
        return 0
    fi

    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$_target_file" | awk '{print $1}'
        return 0
    fi

    if command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$_target_file" | sed 's/^.*= //'
        return 0
    fi

    fail "unable to compute sha256: no supported hashing tool"
}

write_fake_cli() {
    _fake_path=$1
    cat >"$_fake_path" <<'EOF'
#!/bin/sh
set -eu

mode=${FAKE_MODE:-safe}
subcmd=${1:-}

case "$mode:$subcmd" in
    inject:add)
        printf "export PATH='/usr/bin'; touch '%s'\n" "$PWN_FILE"
        ;;
    suspicious:add)
        printf "export PATH='/usr/bin;echo suspicious'\n"
        ;;
    *:list)
        printf 'stored entries\n'
        ;;
    *)
        printf "export PATH='/usr/bin:/bin'\n"
        ;;
esac
EOF
    chmod +x "$_fake_path"
}

test_relative_path_cli_bin_rejected() {
    tmp_dir=$(mktemp -d)
    stderr_file="$tmp_dir/stderr.log"
    status_file="$tmp_dir/status"

    REPO_ROOT="$REPO_ROOT" PATH_CLI_BIN="target/release/path" STDERR_FILE="$stderr_file" STATUS_FILE="$status_file" sh -c '
        cd "$REPO_ROOT"
        . ./path-wrapper.sh >/dev/null 2>>"$STDERR_FILE" || true
        if path list >/dev/null 2>>"$STDERR_FILE"; then
            printf "0\n" >"$STATUS_FILE"
        else
            printf "%s\n" "$?" >"$STATUS_FILE"
        fi
    '

    status=$(cat "$status_file")
    [ "$status" -ne 0 ] || fail "relative PATH_CLI_BIN unexpectedly succeeded"
    grep -q "PATH_CLI_BIN must be absolute" "$stderr_file" || fail "missing warning for relative PATH_CLI_BIN"

    rm -rf "$tmp_dir"
    pass "relative PATH_CLI_BIN is rejected"
}

test_allowlist_rejects_untrusted_binary() {
    tmp_dir=$(mktemp -d)
    fake_bin="$tmp_dir/path"
    stderr_file="$tmp_dir/stderr.log"
    status_file="$tmp_dir/status"

    write_fake_cli "$fake_bin"

    REPO_ROOT="$REPO_ROOT" PATH_CLI_BIN="$fake_bin" PATH_CLI_ALLOWLIST="/definitely/not/trusted" STDERR_FILE="$stderr_file" STATUS_FILE="$status_file" sh -c '
        cd "$REPO_ROOT"
        . ./path-wrapper.sh >/dev/null 2>>"$STDERR_FILE" || true
        if path list >/dev/null 2>>"$STDERR_FILE"; then
            printf "0\n" >"$STATUS_FILE"
        else
            printf "%s\n" "$?" >"$STATUS_FILE"
        fi
    '

    status=$(cat "$status_file")
    [ "$status" -ne 0 ] || fail "allowlist violation unexpectedly succeeded"
    grep -q "not permitted by PATH_CLI_ALLOWLIST" "$stderr_file" || fail "missing allowlist warning"

    rm -rf "$tmp_dir"
    pass "allowlist rejects untrusted binary"
}

test_allowlist_accepts_trusted_binary() {
    tmp_dir=$(mktemp -d)
    fake_bin="$tmp_dir/path"
    stderr_file="$tmp_dir/stderr.log"
    status_file="$tmp_dir/status"

    write_fake_cli "$fake_bin"

    REPO_ROOT="$REPO_ROOT" PATH_CLI_BIN="$fake_bin" PATH_CLI_ALLOWLIST="$tmp_dir" STDERR_FILE="$stderr_file" STATUS_FILE="$status_file" sh -c '
        cd "$REPO_ROOT"
        . ./path-wrapper.sh >/dev/null 2>>"$STDERR_FILE" || true
        if path list >/dev/null 2>>"$STDERR_FILE"; then
            printf "0\n" >"$STATUS_FILE"
        else
            printf "%s\n" "$?" >"$STATUS_FILE"
        fi
    '

    status=$(cat "$status_file")
    [ "$status" -eq 0 ] || fail "allowlisted binary was rejected"

    rm -rf "$tmp_dir"
    pass "allowlist accepts trusted binary"
}

test_checksum_rejects_mismatch() {
    tmp_dir=$(mktemp -d)
    fake_bin="$tmp_dir/path"
    stderr_file="$tmp_dir/stderr.log"
    status_file="$tmp_dir/status"

    write_fake_cli "$fake_bin"

    REPO_ROOT="$REPO_ROOT" PATH_CLI_BIN="$fake_bin" PATH_CLI_SHA256="0000000000000000000000000000000000000000000000000000000000000000" STDERR_FILE="$stderr_file" STATUS_FILE="$status_file" sh -c '
        cd "$REPO_ROOT"
        . ./path-wrapper.sh >/dev/null 2>>"$STDERR_FILE" || true
        if path list >/dev/null 2>>"$STDERR_FILE"; then
            printf "0\n" >"$STATUS_FILE"
        else
            printf "%s\n" "$?" >"$STATUS_FILE"
        fi
    '

    status=$(cat "$status_file")
    [ "$status" -ne 0 ] || fail "checksum mismatch unexpectedly succeeded"
    grep -q "checksum mismatch" "$stderr_file" || fail "missing checksum mismatch warning"

    rm -rf "$tmp_dir"
    pass "checksum mismatch is rejected"
}

test_checksum_accepts_match() {
    tmp_dir=$(mktemp -d)
    fake_bin="$tmp_dir/path"
    stderr_file="$tmp_dir/stderr.log"
    status_file="$tmp_dir/status"

    write_fake_cli "$fake_bin"
    checksum=$(sha256_file "$fake_bin")

    REPO_ROOT="$REPO_ROOT" PATH_CLI_BIN="$fake_bin" PATH_CLI_SHA256="$checksum" STDERR_FILE="$stderr_file" STATUS_FILE="$status_file" sh -c '
        cd "$REPO_ROOT"
        . ./path-wrapper.sh >/dev/null 2>>"$STDERR_FILE" || true
        if path list >/dev/null 2>>"$STDERR_FILE"; then
            printf "0\n" >"$STATUS_FILE"
        else
            printf "%s\n" "$?" >"$STATUS_FILE"
        fi
    '

    status=$(cat "$status_file")
    [ "$status" -eq 0 ] || fail "matching checksum was rejected"

    rm -rf "$tmp_dir"
    pass "matching checksum is accepted"
}

test_injection_output_is_not_executed() {
    tmp_dir=$(mktemp -d)
    fake_bin="$tmp_dir/path"
    pwn_file="$tmp_dir/pwned"
    stderr_file="$tmp_dir/stderr.log"
    status_file="$tmp_dir/status"
    path_after_file="$tmp_dir/path_after"

    write_fake_cli "$fake_bin"

    REPO_ROOT="$REPO_ROOT" PATH_CLI_BIN="$fake_bin" FAKE_MODE="inject" PWN_FILE="$pwn_file" STDERR_FILE="$stderr_file" STATUS_FILE="$status_file" PATH_AFTER_FILE="$path_after_file" sh -c '
        cd "$REPO_ROOT"
        . ./path-wrapper.sh >/dev/null 2>>"$STDERR_FILE" || true
        PATH="/usr/bin:/bin"
        if path add /tmp demo >/dev/null 2>>"$STDERR_FILE"; then
            printf "0\n" >"$STATUS_FILE"
        else
            printf "%s\n" "$?" >"$STATUS_FILE"
        fi
        printf "%s\n" "$PATH" >"$PATH_AFTER_FILE"
    '

    status=$(cat "$status_file")
    [ "$status" -ne 0 ] || fail "malicious output unexpectedly succeeded"
    [ ! -e "$pwn_file" ] || fail "malicious command text was executed"
    [ "$(cat "$path_after_file")" = "/usr/bin:/bin" ] || fail "PATH changed on rejected malicious output"
    if ! grep -Eq "unexpected PATH update format|failed to parse shell-escaped PATH value" "$stderr_file"; then
        fail "missing malformed output warning"
    fi

    rm -rf "$tmp_dir"
    pass "malicious output is rejected without execution"
}

test_suspicious_payload_warns_without_eval() {
    tmp_dir=$(mktemp -d)
    fake_bin="$tmp_dir/path"
    stderr_file="$tmp_dir/stderr.log"
    status_file="$tmp_dir/status"
    path_after_file="$tmp_dir/path_after"

    write_fake_cli "$fake_bin"

    REPO_ROOT="$REPO_ROOT" PATH_CLI_BIN="$fake_bin" FAKE_MODE="suspicious" STDERR_FILE="$stderr_file" STATUS_FILE="$status_file" PATH_AFTER_FILE="$path_after_file" sh -c '
        cd "$REPO_ROOT"
        . ./path-wrapper.sh >/dev/null 2>>"$STDERR_FILE" || true
        PATH="/usr/bin:/bin"
        if path add /tmp demo >/dev/null 2>>"$STDERR_FILE"; then
            printf "0\n" >"$STATUS_FILE"
        else
            printf "%s\n" "$?" >"$STATUS_FILE"
        fi
        printf "%s\n" "$PATH" >"$PATH_AFTER_FILE"
    '

    status=$(cat "$status_file")
    [ "$status" -eq 0 ] || fail "suspicious payload was rejected unexpectedly"
    [ "$(cat "$path_after_file")" = "/usr/bin;echo suspicious" ] || fail "PATH payload was not applied as expected"
    grep -q "shell metacharacters; applying without eval" "$stderr_file" || fail "missing suspicious payload warning"

    rm -rf "$tmp_dir"
    pass "suspicious payload triggers warning without eval"
}

test_relative_path_cli_bin_rejected
test_allowlist_rejects_untrusted_binary
test_allowlist_accepts_trusted_binary
test_checksum_rejects_mismatch
test_checksum_accepts_match
test_injection_output_is_not_executed
test_suspicious_payload_warns_without_eval

printf '%s\n' "All path-wrapper security tests passed."
