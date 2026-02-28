#![deny(warnings)]

// Integration tests exercising the command-line interface.  We
// deliberately run the compiled binary and manipulate the temporary
// working directory and environment variables instead of calling internal
// functions, as this ensures the CLI remains stable.
use assert_cmd::cargo;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

/// Confirm that, with no subcommands, the utility simply echoes the
/// value of the `PATH` environment variable.
#[test]
fn prints_path_env() {
    // run in an isolated directory so the workspace's `.path` file is
    // ignored; earlier versions of the test ran from the repo root which
    // now triggers validation errors when the file contains malformed lines.
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir);
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

    let mut cmd = cargo::cargo_bin_cmd!("path");
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

    let mut cmd = cargo::cargo_bin_cmd!("path");
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
    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir).env("PATH", "");
    cmd.arg("add").arg("/foo").arg("foo");
    cmd.assert().success();

    let mut cmd2 = cargo::cargo_bin_cmd!("path");
    cmd2.current_dir(&dir).env("PATH", "");
    cmd2.arg("add").arg("/bar");
    cmd2.assert().success();

    // run list and inspect output; /bar should not appear because it had no name
    let mut list_cmd = cargo::cargo_bin_cmd!("path");
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

/// If `.path` contains entries whose locations don't exist, the tool should
/// warn about them when started but leave the file alone.
#[test]
fn invalid_entries_are_warned_about() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    // write one invalid and one valid line
    fs::write(&store, "/no/such/thing\tbad\n/tmp\ttmp\n").unwrap();

    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir).env("PATH", "");
    cmd.arg("list");
    let assert = cmd.assert().success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("warning: the following stored paths do not exist"));
    assert!(stderr.contains("/no/such/thing"));

    // store file should remain unchanged
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("/no/such/thing"));
    assert!(contents.contains("/tmp\ttmp"));
}

/// Entries without a name (either explicitly empty or because the line
/// was malformed with no tab separator) cause the program to abort and
/// report the offending line along with its line number.
#[test]
fn nameless_entry_causes_error() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    // one line has an empty name, the other is completely lacking the tab
    fs::write(&store, "/some/path\t\n/foo/foo\n/foo\tfoo\n").unwrap();

    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir).env("PATH", "");
    let assert = cmd.arg("list").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: found nameless entry"));
    // should include the line number (line 1 has /some/path\t\n)
    assert!(stderr.contains("at line 1"));
    // should include the location
    assert!(stderr.contains("/some/path") || stderr.contains("/foo/foo"));
    // file should remain untouched
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("/some/path"));
    assert!(contents.contains("/foo/foo"));
    assert!(contents.contains("/foo\tfoo"));
}

/// Attempting to add an entry with a name that already exists should fail.
#[test]
fn duplicate_names_are_rejected() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    // first add with name "myname"
    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir).env("PATH", "");
    cmd.arg("add").arg("/tmp").arg("myname");
    cmd.assert().success();

    // second add with the same name "myname" should fail
    let mut cmd2 = cargo::cargo_bin_cmd!("path");
    cmd2.current_dir(&dir).env("PATH", "");
    cmd2.arg("add").arg("/usr/local/bin").arg("myname");
    let assert = cmd2.assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: name 'myname' is already in use"));

    // verify that only the first entry was stored
    let store = dir.join(".path");
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("/tmp\tmyname"));
    assert!(!contents.contains("/usr/local/bin"));
}

/// Entries with duplicate names in the .path file should cause the program
/// to abort and report which lines have the duplicated name.
#[test]
fn duplicate_names_in_file_cause_error() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    // write two entries with the same name "dup"
    fs::write(&store, "/foo/a\tdup\n/foo/b\tdup\n/foo/c\tunique\n").unwrap();

    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir).env("PATH", "");
    let assert = cmd.arg("list").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: duplicate name 'dup'"));
    assert!(stderr.contains("lines: 1, 2"));
    // file should remain untouched
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("/foo/a\tdup"));
    assert!(contents.contains("/foo/b\tdup"));
    assert!(contents.contains("/foo/c\tunique"));
}

/// Names with non-alphanumeric characters should be rejected.
#[test]
fn invalid_names_are_rejected() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    // try adding with a name containing spaces
    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir).env("PATH", "");
    cmd.arg("add").arg("/tmp").arg("my name");
    let assert = cmd.assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: invalid name 'my name'"));
    assert!(stderr.contains("alphanumeric characters"));
}

/// Invalid names in the .path file should cause the program to abort.
#[test]
fn invalid_names_in_file_cause_error() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    // write an entry with an invalid name (contains a dash)
    fs::write(&store, "/foo/a\tvalid123\n/foo/b\tinvalid-name\n").unwrap();

    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir).env("PATH", "");
    let assert = cmd.arg("list").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: invalid name 'invalid-name'"));
    assert!(stderr.contains("alphanumeric characters"));
    // file should remain untouched
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("valid123"));
    assert!(contents.contains("invalid-name"));
}

/// Adding by name should look up the stored path and add it to PATH.
#[test]
fn add_by_stored_name() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    // first add with name "mytools"
    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir).env("PATH", "");
    cmd.arg("add").arg("/usr/local/bin").arg("mytools");
    cmd.assert().success();

    // now add by name "mytools" (should look it up and add /usr/local/bin)
    let mut cmd2 = cargo::cargo_bin_cmd!("path");
    cmd2.current_dir(&dir).env("PATH", "");
    cmd2.arg("add").arg("mytools");
    let output = cmd2.assert().success().get_output().stdout.clone();
    let out_str = String::from_utf8_lossy(&output);
    // should print path containing /usr/local/bin
    assert!(out_str.contains("/usr/local/bin"));
}

/// When a directory with the same name exists in the working directory,
/// supplying that name to `add` should still resolve via the stored entry
/// rather than treating the string as a filesystem path.
#[test]
fn name_precedence_over_actual_path() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    // create a real directory called "x"
    fs::create_dir(dir.join("x")).unwrap();

    // store an entry named "x" that points somewhere else
    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir).env("PATH", "");
    cmd.arg("add").arg("/real/location").arg("x");
    cmd.assert().success();

    // now run `add x` -- it should use the stored path, not ./x
    let mut cmd2 = cargo::cargo_bin_cmd!("path");
    cmd2.current_dir(&dir).env("PATH", "");
    let output = cmd2
        .arg("add")
        .arg("x")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    let actual_path = dir.join("x");
    let actual = actual_path.to_string_lossy();
    assert!(out_str.contains("/real/location"));
    assert!(!out_str.contains(actual.as_ref()));
}

/// Paths passed to `add` must either be absolute or start with a dot.
#[test]
fn enforce_path_format_for_add() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir).env("PATH", "");
    cmd.arg("add").arg("notvalid");
    let assert = cmd.assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("must be absolute or start with '.'"));

    // starting with '.' is fine
    let mut cmd2 = cargo::cargo_bin_cmd!("path");
    cmd2.current_dir(&dir).env("PATH", "");
    cmd2.arg("add").arg("./rel");
    cmd2.assert().success();
}

/// Adding a path that refers to a regular file (not a directory) should fail.
#[test]
fn reject_file_locations() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    // create an actual file
    let file = dir.join("f.txt");
    fs::write(&file, "hello").unwrap();

    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(&dir).env("PATH", "");
    cmd.arg("add").arg("./f.txt");
    let assert = cmd.assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("exists but is not a directory"));
}
