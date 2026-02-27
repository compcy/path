// Integration tests exercising the command-line interface.  We
// deliberately run the compiled binary and manipulate the temporary
// working directory and environment variables instead of calling internal
// functions, as this ensures the CLI remains stable.
use assert_cmd::Command;
use predicates::prelude::*;
use std::env;
use std::fs;
use tempfile::tempdir;

/// Confirm that, with no subcommands, the utility simply echoes the
/// value of the `PATH` environment variable.
#[test]
fn prints_path_env() {
    let mut cmd = Command::cargo_bin("path").unwrap();
    cmd.env("PATH", "foo:bar");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("foo:bar"));
}

/// Verify that `path add <location>` modifies the path string by
/// appending, and that when no name is provided nothing is recorded.
#[test]
fn add_appends_but_only_records_with_name() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = Command::cargo_bin("path").unwrap();
    cmd.current_dir(&dir);
    cmd.env("PATH", "A");
    cmd.arg("add").arg("/tmp/x");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("A:/tmp/x"));

    // verify store file does not contain an entry
    let store = dir.join(".path");
    let contents = fs::read_to_string(store).unwrap_or_default();
    assert!(!contents.contains("/tmp/x"));
}

/// Ensure that providing a name and the `--pre` flag prepends the
/// location and stores the supplied name (only named entries are stored).
#[test]
fn add_with_name_and_prepend() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = Command::cargo_bin("path").unwrap();
    cmd.current_dir(&dir);
    cmd.env("PATH", "B");
    cmd.arg("add").arg("--pre").arg("/tmp/y").arg("yname");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("/tmp/y:B"));

    let store = dir.join(".path");
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("/tmp/y\tyname"));
}

/// After adding entries, `list` should print them in a readable form.
#[test]
fn list_shows_entries() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    // add two entries; only the named one should be stored
    let mut cmd = Command::cargo_bin("path").unwrap();
    cmd.current_dir(&dir).env("PATH", "");
    cmd.arg("add").arg("/foo").arg("foo");
    cmd.assert().success();

    let mut cmd2 = Command::cargo_bin("path").unwrap();
    cmd2.current_dir(&dir).env("PATH", "");
    cmd2.arg("add").arg("/bar");
    cmd2.assert().success();

    // run list and inspect output; /bar should not appear because it had no name
    let mut list_cmd = Command::cargo_bin("path").unwrap();
    list_cmd.current_dir(&dir).env("PATH", "");
    let output = list_cmd
        .arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("/foo (foo)"));
    assert!(!out_str.contains("/bar"));
}
