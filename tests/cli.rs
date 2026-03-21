#![deny(warnings)]

// Integration tests exercising the command-line interface.  We
// deliberately run the compiled binary and manipulate the temporary
// working directory and environment variables instead of calling internal
// functions, as this ensures the CLI remains stable.
use assert_cmd::cargo;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

/// Helper to construct a test command with standard directory and store file setup.
fn test_cmd(dir: &Path, path_env: &str) -> assert_cmd::Command {
    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(dir)
        .arg("--file")
        .arg(dir.join(".path"))
        .env("PATH", path_env);
    cmd
}

/// Returns the path to an existing fixture file under `tests/paths`.
fn fixture_path(fixture_name: &str) -> PathBuf {
    Path::new("tests")
        .join("paths")
        .join(format!("{}.path", fixture_name))
}

/// Copies an existing fixture file into the temp test directory as `.path`.
///
/// This keeps fixture data source-of-truth in `tests/paths/*.path` while
/// ensuring each test runs against an isolated, writable store file.
fn copy_fixture_to_temp_store(dir: &Path, fixture_name: &str) -> io::Result<()> {
    fs::copy(fixture_path(fixture_name), dir.join(".path")).map(|_| ())
}

/// Runs a command that is expected to succeed and returns stdout as UTF-8 text.
fn get_stdout(cmd: &mut assert_cmd::Command) -> String {
    let output = cmd.assert().success().get_output().stdout.clone();
    String::from_utf8_lossy(&output).to_string()
}

/// Runs a command that is expected to fail and returns stderr as UTF-8 text.
fn get_stderr(cmd: &mut assert_cmd::Command) -> String {
    let assert = cmd.assert().failure();
    String::from_utf8_lossy(&assert.get_output().stderr).to_string()
}

/// Confirm that, with no subcommands, the utility simply echoes the
/// value of the `PATH` environment variable.
#[test]
fn prints_path_env() {
    // run in an isolated directory so the workspace's `.path` file is
    // ignored; earlier versions of the test ran from the repo root which
    // now triggers validation errors when the file contains malformed lines.
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "foo:bar");
    let output = cmd.assert().success().get_output().stdout.clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("export PATH='"));
    assert!(out_str.contains("foo:bar"));
}

/// By default, entries are persisted to `$HOME/.path` when `--file` is omitted.
#[test]
fn default_store_file_is_home_dot_path() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let home = dir.join("home");
    fs::create_dir(&home).unwrap();

    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(dir).env("HOME", &home).env("PATH", "");
    cmd.arg("add").arg("/tmp/home-default").arg("homeentry");
    cmd.assert().success();

    let contents = fs::read_to_string(home.join(".path")).unwrap();
    assert!(contents.contains("'/tmp/home-default' [homeentry] (auto)"));
}

/// Creating a new store file should write a first-line layout comment.
#[test]
fn add_writes_layout_comment_when_creating_store_file() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");

    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg("/tmp/layout").arg("layout");
    cmd.assert().success();

    let contents = fs::read_to_string(store).unwrap();
    let mut lines = contents.lines();
    assert_eq!(
        lines.next(),
        Some("# layout: '<location>' [<name>] (<options>)")
    );
    assert_eq!(lines.next(), Some("'/tmp/layout' [layout] (auto)"));
}

/// The store-file option is long-only; `-f` is reserved for future use.
#[test]
fn short_f_is_not_accepted() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = cargo::cargo_bin_cmd!("path");
    cmd.current_dir(dir).env("PATH", "");
    let assert = cmd
        .arg("-f")
        .arg("/tmp/custom.path")
        .arg("list")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("-f"));
}

/// Verify that `path add <location>` modifies the path string by
/// appending, and that when no name is provided nothing is recorded.
#[test]
fn add_appends_but_only_records_with_name() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "A");
    let output = cmd
        .arg("add")
        .arg("/tmp/x")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("export PATH='"));
    assert!(out_str.contains("A:/tmp/x"));

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

    let mut cmd = test_cmd(dir, "B");
    let output = cmd
        .arg("add")
        .arg("--pre")
        .arg("/tmp/y")
        .arg("yname")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("export PATH='"));
    assert!(out_str.contains("/tmp/y:B"));

    let store = dir.join(".path");
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("'/tmp/y' [yname] (auto,pre)"));
}

/// Adding a location containing spaces should be stored and exported correctly.
#[test]
fn add_with_spaced_location_round_trips_through_store_and_output() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    fs::create_dir(dir.join("my tools")).unwrap();
    let canonical = fs::canonicalize(dir.join("my tools"))
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let mut cmd = test_cmd(dir, "/usr/bin");
    cmd.arg("add").arg("./my tools").arg("mytools");
    let out_str = get_stdout(&mut cmd);
    assert_eq!(
        out_str.trim(),
        format!("export PATH='/usr/bin:{}'", canonical)
    );

    let store = dir.join(".path");
    let contents = fs::read_to_string(store).unwrap();
    let escaped_canonical = canonical.replace('\\', "\\\\").replace('\'', "\\'");
    assert!(contents.contains(&format!("'{}' [mytools] (auto)", escaped_canonical)));

    let mut cmd2 = test_cmd(dir, "");
    let output2 = cmd2
        .arg("add")
        .arg("mytools")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str2 = String::from_utf8_lossy(&output2);
    assert_eq!(out_str2.trim(), format!("export PATH='{}'", canonical));
}

/// After adding entries, `list` should print them in a readable form.
#[test]
fn list_shows_entries() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    // add two entries; only the named one should be stored
    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg("/foo").arg("foo");
    cmd.assert().success();

    let mut cmd2 = test_cmd(dir, "");
    cmd2.arg("add").arg("/bar");
    cmd2.assert().success();

    // run list and inspect output; /bar should not appear because it had no name
    let mut list_cmd = test_cmd(dir, "");
    let output = list_cmd
        .arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("/foo [foo] (auto)"));
    assert!(!out_str.contains("/bar"));
}

/// Entries stored as `noauto` should display their status in list output.
#[test]
fn list_shows_noauto_status() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    copy_fixture_to_temp_store(dir, "auto_noauto").unwrap();

    let mut cmd = test_cmd(dir, "");
    let output = cmd
        .arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("/opt/auto [a] (auto)"));
    assert!(out_str.contains("/opt/no [n] (noauto)"));
}

/// `list` should print a message when the configured store file is missing.
#[test]
fn list_reports_missing_store_file() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    let output = cmd
        .arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("No stored entries: store file does not exist."));
}

/// `list` should print a message when the store file exists but has no entries.
#[test]
fn list_reports_empty_store_file() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "").unwrap();

    let mut cmd = test_cmd(dir, "");
    let output = cmd
        .arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("No stored entries."));
}

/// If `.path` contains entries whose locations don't exist, the tool should
/// warn about them when started but leave the file alone.
#[test]
fn invalid_entries_are_warned_about() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    copy_fixture_to_temp_store(dir, "one_invalid_one_valid").unwrap();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("list");
    let assert = cmd.assert().success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("warning: the following stored paths do not exist"));
    assert!(stderr.contains("/no/such/thing"));

    // store file should remain unchanged
    let contents = fs::read_to_string(dir.join(".path")).unwrap();
    assert!(contents.contains("/no/such/thing"));
    assert!(contents.contains("'/tmp' [tmp] (auto)"));
}

/// Entries without a name (either explicitly empty or because the line
/// was malformed with no field separator) cause the program to abort and
/// report the offending line along with its line number.
#[test]
fn nameless_entry_causes_error() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    // one line uses an unwrapped name, the other has no field separator
    fs::write(
        &store,
        "'/some/path' bad (auto)\n'/foo/foo'\n'/foo' [foo] (auto)\n",
    )
    .unwrap();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("list").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: found nameless entry"));
    // should include the line number (line 1 has /some/path with no name)
    assert!(stderr.contains("at line 1"));
    // should include the location
    assert!(stderr.contains("/some/path") || stderr.contains("/foo/foo"));
    // file should remain untouched
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("/some/path"));
    assert!(contents.contains("/foo/foo"));
    assert!(contents.contains("'/foo' [foo] (auto)"));
}

/// Attempting to add an entry with a name that already exists should fail.
#[test]
fn duplicate_names_are_rejected() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    // first add with name "myname"
    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg("/tmp").arg("myname");
    cmd.assert().success();

    // second add with the same name "myname" should fail
    let mut cmd2 = test_cmd(dir, "");
    cmd2.arg("add").arg("/usr/local/bin").arg("myname");
    let assert = cmd2.assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: name 'myname' is already in use"));

    // verify that only the first entry was stored
    let store = dir.join(".path");
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("'/tmp' [myname] (auto)"));
    assert!(!contents.contains("/usr/local/bin"));
}

/// Entries with duplicate names in the .path file should cause the program
/// to abort and report which lines have the duplicated name.
#[test]
fn duplicate_names_in_file_cause_error() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    copy_fixture_to_temp_store(dir, "duplicate_names").unwrap();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("list").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: duplicate name 'dup'"));
    assert!(stderr.contains("lines: 2, 3"));
    // file should remain untouched
    let contents = fs::read_to_string(dir.join(".path")).unwrap();
    assert!(contents.contains("'/foo/a' [dup] (auto)"));
    assert!(contents.contains("'/foo/b' [dup] (auto)"));
    assert!(contents.contains("'/foo/c' [unique] (auto)"));
}

/// Names with non-alphanumeric characters should be rejected.
#[test]
fn invalid_names_are_rejected() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    // try adding with a name containing spaces
    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg("/tmp").arg("my name");
    let stderr = get_stderr(&mut cmd);
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
    fs::write(
        &store,
        "'/foo/a' [valid123] (auto)\n'/foo/b' [invalid-name] (auto)\n",
    )
    .unwrap();

    let mut cmd = test_cmd(dir, "");
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
    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg("/usr/local/bin").arg("mytools");
    cmd.assert().success();

    // now add by name "mytools" (should look it up and add /usr/local/bin)
    let mut cmd2 = test_cmd(dir, "");
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
    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg("/real/location").arg("x");
    cmd.assert().success();

    // now run `add x` -- it should use the stored path, not ./x
    let mut cmd2 = test_cmd(dir, "");
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

    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg("notvalid");
    let stderr = get_stderr(&mut cmd);
    assert!(stderr.contains("must be absolute or start with '.'"));

    // starting with '.' is fine
    let mut cmd2 = test_cmd(dir, "");
    cmd2.arg("add").arg("./rel");
    cmd2.assert().success();
}

/// Paths passed to `add` must not contain `:`.
#[test]
fn add_rejects_colon_in_path_argument() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg("/tmp:evil");
    let stderr = get_stderr(&mut cmd);
    assert!(stderr.contains("must not contain ':'"));
}

/// Paths passed to `add` must not contain `\\`.
#[test]
fn add_rejects_backslash_in_path_argument() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg("/tmp\\evil");
    let stderr = get_stderr(&mut cmd);
    assert!(stderr.contains("must not contain '\\\\'"));
}

/// Adding a path that refers to a regular file (not a directory) should fail.
#[test]
fn reject_file_locations() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    // create an actual file
    let file = dir.join("f.txt");
    fs::write(&file, "hello").unwrap();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg("./f.txt");
    let stderr = get_stderr(&mut cmd);
    assert!(stderr.contains("exists but is not a directory"));
}

/// `remove` should only affect the PATH output and not delete entries from `.path`.
#[test]
fn remove_keeps_store_entries() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "'/tmp' [home] (auto)\n").unwrap();

    let mut cmd = test_cmd(dir, "/tmp:/usr/bin");
    let output = cmd
        .arg("remove")
        .arg("home")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(!out_str.contains("/tmp"));
    assert!(out_str.contains("/usr/bin"));

    // store entry should remain
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("'/tmp' [home] (auto)"));
}

/// `remove` should reject stored entries marked `protect`.
#[test]
fn remove_rejects_protected_store_entry() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "'/tmp' [home] (auto,protect)\n").unwrap();

    let mut cmd = test_cmd(dir, "/tmp:/usr/bin");
    let assert = cmd.arg("remove").arg("home").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("protected"));

    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("'/tmp' [home] (auto,protect)"));
}

/// `remove` should reject direct path removal for stored entries marked `protect`.
#[test]
fn remove_rejects_protected_store_path_argument() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "'/tmp' [home] (auto,protect)\n").unwrap();

    let mut cmd = test_cmd(dir, "/tmp:/usr/bin");
    let assert = cmd.arg("remove").arg("/tmp").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("protected"));

    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("'/tmp' [home] (auto,protect)"));
}

/// `delete` should remove the matching entry from `.path` by name.
#[test]
fn delete_removes_store_entry_by_name() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "'/tmp' [home] (auto)\n'/usr/bin' [sys] (auto)\n").unwrap();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("delete").arg("home");
    cmd.assert().success();

    let contents = fs::read_to_string(store).unwrap();
    assert!(!contents.contains("'/tmp' [home] (auto)"));
    assert!(contents.contains("'/usr/bin' [sys] (auto)"));
}

/// `remove` by path should match both canonicalized and raw PATH segments.
#[test]
fn remove_by_path_matches_raw_segment_too() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    fs::create_dir(dir.join("rel")).unwrap();

    let mut cmd = test_cmd(dir, "./rel:/usr/bin");
    let output = cmd
        .arg("remove")
        .arg("./rel")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert_eq!(out_str.trim(), "export PATH='/usr/bin'");
}

/// Paths passed to `remove` must not contain `:`.
#[test]
fn remove_rejects_colon_in_path_argument() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("remove").arg("/tmp:evil");
    let stderr = get_stderr(&mut cmd);
    assert!(stderr.contains("must not contain ':'"));
}

/// Paths passed to `remove` must not contain `\\`.
#[test]
fn remove_rejects_backslash_in_path_argument() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("remove").arg("/tmp\\evil");
    let stderr = get_stderr(&mut cmd);
    assert!(stderr.contains("must not contain '\\\\'"));
}

/// `list` should fail if the store cannot be loaded.
#[test]
fn list_fails_when_store_is_unreadable() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    fs::create_dir(dir.join(".path")).unwrap();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("list").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: failed to load entries"));
}

/// `delete` should fail if the store cannot be loaded.
#[test]
fn delete_fails_when_store_is_unreadable() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    fs::create_dir(dir.join(".path")).unwrap();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("delete").arg("home").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: failed to load entries"));
}

/// Paths passed to `delete` must not contain `:`.
#[test]
fn delete_rejects_colon_in_path_argument() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("delete").arg("/tmp:evil");
    let stderr = get_stderr(&mut cmd);
    assert!(stderr.contains("must not contain ':'"));
}

/// Paths passed to `delete` must not contain `\\`.
#[test]
fn delete_rejects_backslash_in_path_argument() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("delete").arg("/tmp\\evil");
    let stderr = get_stderr(&mut cmd);
    assert!(stderr.contains("must not contain '\\\\'"));
}

/// Adding `.` should use the canonical current directory in PATH output.
#[test]
fn add_dot_uses_canonical_path_in_output() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let canonical = fs::canonicalize(dir)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg(".").arg("dot");
    let out_str = get_stdout(&mut cmd);
    assert_eq!(out_str.trim(), format!("export PATH='{}'", canonical));
}

/// Adding `.` when PATH already contains the canonical cwd should not duplicate it.
#[test]
fn add_dot_does_not_duplicate_existing_entry() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let canonical = fs::canonicalize(dir)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let mut cmd = test_cmd(dir, &canonical);
    cmd.arg("add").arg(".").arg("dot");
    let out_str = get_stdout(&mut cmd);
    assert_eq!(out_str.trim(), format!("export PATH='{}'", canonical));
}

/// Trailing slash variants are treated as equivalent and are not duplicated.
#[test]
fn add_does_not_duplicate_trailing_slash_variant() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let canonical = fs::canonicalize(dir)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let mut with_slash = canonical.clone();
    with_slash.push('/');
    let initial_path = format!("{}:/usr/bin", with_slash);

    let mut cmd = test_cmd(dir, &initial_path);
    cmd.arg("add").arg(".").arg("dot");
    let out_str = get_stdout(&mut cmd);
    assert_eq!(
        out_str.trim(),
        format!("export PATH='{}:/usr/bin'", with_slash)
    );
}

/// Stored paths with trailing slashes should be normalized when read from file.
#[test]
fn list_normalizes_trailing_slash_from_store_file() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "'/opt/tools/' [tools] (auto)\n").unwrap();

    let mut cmd = test_cmd(dir, "");
    let output = cmd
        .arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("/opt/tools [tools] (auto)"));
    assert!(!out_str.contains("/opt/tools/ [tools] (auto)"));
}

/// Stored relative locations should be rejected during startup validation.
#[test]
fn relative_stored_location_causes_error() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "'./rel' [rel] (auto)\n").unwrap();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("list").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: invalid stored location './rel'"));
}

/// Stored absolute paths containing parent traversal should be rejected.
#[test]
fn noncanonical_stored_location_causes_error() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "'/tmp/../tmp' [bad] (auto)\n").unwrap();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("list").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: invalid stored location '/tmp/../tmp'"));
}

/// Stored locations containing `:` should be rejected during startup validation.
#[test]
fn stored_location_with_colon_causes_error() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "'/tmp:evil' [bad] (auto)\n").unwrap();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("list").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: invalid stored location '/tmp:evil'"));
}

/// Delimiter-malicious and asymmetrical store-file cases should be rejected.
#[test]
fn list_rejects_delimiter_malicious_cases() {
    let cases = [
        "malicious/location_parentheses",
        "malicious/location_open_parenthesis",
        "malicious/location_close_parenthesis",
        "malicious/location_braces",
        "malicious/location_open_brace",
        "malicious/location_close_brace",
        "malicious/location_square_brackets",
        "malicious/location_open_bracket",
        "malicious/location_close_bracket",
        "malicious/location_escaped_close_bracket",
        "malicious/location_escaped_close_parenthesis",
        "malicious/location_escaped_close_brace",
        "malicious/name_open_bracket",
        "malicious/name_close_bracket",
        "malicious/name_open_parenthesis",
        "malicious/name_close_parenthesis",
        "malicious/name_open_brace",
        "malicious/name_close_brace",
        "malicious/name_missing_closing_bracket",
        "malicious/name_missing_opening_bracket",
        "malicious/name_empty_brackets",
        "malicious/options_open_bracket",
        "malicious/options_close_bracket",
        "malicious/options_open_parenthesis",
        "malicious/options_close_parenthesis",
        "malicious/options_open_brace",
        "malicious/options_close_brace",
        "malicious/options_nested_braces",
        "malicious/options_nested_brackets",
        "malicious/options_nested_parentheses",
        "malicious/options_missing_closing_parenthesis",
        "malicious/options_missing_opening_parenthesis",
        "malicious/location_backtick",
        "malicious/location_asymmetric_backtick",
        "malicious/name_backtick",
        "malicious/options_backtick",
        "malicious/location_semicolon",
        "malicious/location_dollar",
        "malicious/location_pipe",
        "malicious/location_wildcard_star",
        "malicious/location_wildcard_question",
        "malicious/location_ampersand",
        "malicious/location_redirect_less",
        "malicious/location_redirect_greater",
        "malicious/location_hash",
    ];

    for fixture_name in cases {
        let temp = tempdir().unwrap();
        let dir = temp.path();
        copy_fixture_to_temp_store(dir, fixture_name).unwrap();

        let mut cmd = test_cmd(dir, "");
        let assert = cmd.arg("list").assert().failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        assert!(
            stderr.contains("error:"),
            "fixture '{}' expected stderr to contain an error, got: {}",
            fixture_name,
            stderr
        );
    }
}

/// Adding with `--noauto` should persist `noauto` in the third field.
#[test]
fn add_with_noauto_stores_noauto_marker() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("add")
        .arg("--noauto")
        .arg("/tmp/noauto")
        .arg("noautoentry");
    cmd.assert().success();

    let store = dir.join(".path");
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("'/tmp/noauto' [noautoentry] (noauto)"));
}

/// Adding with `--protect` should persist `protect` in the third field.
#[test]
fn add_with_protect_stores_protect_marker() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("add")
        .arg("--protect")
        .arg("/tmp/protect")
        .arg("protectentry");
    cmd.assert().success();

    let store = dir.join(".path");
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("'/tmp/protect' [protectentry] (auto,protect)"));
}

/// `restore` should add the standard protected system paths to PATH.
#[test]
fn restore_adds_standard_system_paths_to_path() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    let output = cmd
        .arg("restore")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);

    assert_eq!(
        out_str.trim(),
        "export PATH='/bin:/sbin:/usr/bin:/usr/sbin:/usr/local/bin:/usr/local/sbin'"
    );
}

/// `restore` should not persist the built-in protected system paths to the store file.
#[test]
fn restore_does_not_persist_system_paths_to_store() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("restore");
    cmd.assert().success();

    assert!(!dir.join(".path").exists());
}

/// `restore` should be idempotent when system paths are already present.
#[test]
fn restore_does_not_duplicate_existing_system_paths() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd: assert_cmd::Command = test_cmd(
        dir,
        "/bin:/sbin:/usr/bin:/usr/sbin:/usr/local/bin:/usr/local/sbin",
    );
    let output = cmd
        .arg("restore")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);

    assert_eq!(
        out_str.trim(),
        "export PATH='/bin:/sbin:/usr/bin:/usr/sbin:/usr/local/bin:/usr/local/sbin'"
    );
}

/// `remove` should reject built-in protected system paths by reserved name.
#[test]
fn remove_rejects_builtin_system_path_name() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "/bin:/usr/bin");
    let assert = cmd.arg("remove").arg("sysbin").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("system path '/bin' (sysbin) is protected"));
}

/// `remove` should reject built-in protected system paths by direct path.
#[test]
fn remove_rejects_builtin_system_path_location() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "/bin:/usr/bin");
    let assert = cmd.arg("remove").arg("/bin").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("system path '/bin' (sysbin) is protected"));
}

/// Built-in system path names should be reserved and unavailable for stored entries.
#[test]
fn add_rejects_reserved_system_path_name() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("add").arg("/tmp/tools").arg("sysbin");
    let stderr = get_stderr(&mut cmd);
    assert!(stderr.contains("name 'sysbin' is reserved for a protected system path"));
}

/// `list` should read quoted locations containing literal spaces.
#[test]
fn list_reads_quoted_location_with_spaces() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "'/opt/my tools' [tools] (auto)\n").unwrap();

    let mut cmd = test_cmd(dir, "");
    let output = cmd
        .arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("/opt/my tools [tools] (auto)"));
}

/// `load` should add only `auto` entries and skip `noauto` entries.
#[test]
fn load_adds_only_auto_entries() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(
        &store,
        "'/opt/auto1' [a1] (auto)\n'/opt/noauto' [n1] (noauto)\n'/opt/auto2' [a2] (auto)\n",
    )
    .unwrap();

    let mut cmd = test_cmd(dir, "");
    let output = cmd
        .arg("load")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);

    assert!(out_str.contains("/opt/auto1"));
    assert!(out_str.contains("/opt/auto2"));
    assert!(!out_str.contains("/opt/noauto"));
}

/// `load` should prepend entries marked `pre`; entries without `pre` are appended.
#[test]
fn load_respects_pre_option_with_post_default() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(
        &store,
        "'/opt/pre' [p] (auto,pre)\n'/opt/post' [q] (auto)\n",
    )
    .unwrap();

    let mut cmd = test_cmd(dir, "/usr/bin");
    let output = cmd
        .arg("load")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);

    assert_eq!(out_str.trim(), "export PATH='/opt/pre:/usr/bin:/opt/post'");
}

/// `list` should reject legacy unwrapped store lines.
#[test]
fn list_rejects_legacy_space_separated_entries() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "/opt/tools tools auto\n").unwrap();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("list").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: found nameless entry"));
}

/// `load` should treat a blank third field as `auto` for manually edited files.
#[test]
fn load_treats_blank_third_field_as_auto() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "'/opt/manual' [manual]\n").unwrap();

    let mut cmd = test_cmd(dir, "");
    let output = cmd
        .arg("load")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);

    assert!(out_str.contains("/opt/manual"));
}

/// `verify` should report success when validation passes.
#[test]
fn verify_reports_success_when_entries_are_valid() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");

    let location = dir.to_string_lossy();
    fs::write(&store, format!("'{}' [valid] (auto)\n", location)).unwrap();

    let mut cmd = test_cmd(dir, "");
    let output = cmd
        .arg("verify")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("Path file is valid."));
}

/// `verify` should surface validation failures from the store file.
#[test]
fn verify_surfaces_validation_failures() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "'/foo/a' [dup] (auto)\n'/foo/b' [dup] (auto)\n").unwrap();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("verify").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: duplicate name 'dup'"));
}

/// `verify` should fail when an entry contains an unknown or misspelled option.
#[test]
fn verify_fails_for_unknown_or_misspelled_option() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    for option in ["autoo", "protec"] {
        let store = dir.join(".path");
        fs::write(&store, format!("'/tmp/safe' [safe] ({})\n", option)).unwrap();

        let mut cmd = cargo::cargo_bin_cmd!("path");
        cmd.current_dir(dir)
            .arg("--file")
            .arg(&store)
            .env("PATH", "");
        let assert = cmd.arg("verify").assert().failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        assert!(stderr.contains("error: invalid entry option"));
        assert!(
            stderr.contains(option),
            "stderr should include invalid option '{}': {}",
            option,
            stderr
        );
    }
}

/// `verify` should fail when the configured store file does not exist.
#[test]
fn verify_fails_when_store_file_is_missing() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("verify").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: store file does not exist"));
}

/// `verify` should fail when the configured store file exists but has no entries.
#[test]
fn verify_fails_when_store_file_is_empty() {
    let temp = tempdir().unwrap();
    let dir = temp.path();
    let store = dir.join(".path");
    fs::write(&store, "").unwrap();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("verify").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: store file has no entries"));
}

/// `list --pretty` should print a header row followed by one line per PATH segment.
#[test]
fn list_pretty_shows_header_and_segments() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "/usr/bin:/bin");
    let out = get_stdout(cmd.arg("list").arg("--pretty"));

    // Header row must be present.
    assert!(out.contains("PATH"));
    assert!(out.contains("NAME"));

    // Each PATH segment must appear on its own line.
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.iter().any(|l| l.starts_with("/usr/bin")));
    assert!(lines.iter().any(|l| l.starts_with("/bin")));
}

/// `list --pretty` should resolve names from the built-in system path list.
#[test]
fn list_pretty_resolves_system_path_names() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "/usr/bin:/bin:/sbin");
    let out = get_stdout(cmd.arg("list").arg("--pretty"));

    assert!(out.contains("usrbin"));
    assert!(out.contains("sysbin"));
    assert!(out.contains("syssbin"));
}

/// `list --pretty` should resolve names from stored entries in the store file.
#[test]
fn list_pretty_resolves_stored_entry_names() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    // Store an entry for /tmp so its name shows up in --pretty output.
    let mut add_cmd = test_cmd(dir, "");
    add_cmd.arg("add").arg("/tmp").arg("mytmp");
    add_cmd.assert().success();

    let mut cmd = test_cmd(dir, "/tmp:/usr/bin");
    let out = get_stdout(cmd.arg("list").arg("--pretty"));

    // /tmp should appear with its stored name.
    let tmp_line = out.lines().find(|l| l.starts_with("/tmp")).unwrap();
    assert!(tmp_line.contains("mytmp"));

    // /usr/bin should appear with its built-in system name.
    let usrbin_line = out.lines().find(|l| l.starts_with("/usr/bin")).unwrap();
    assert!(usrbin_line.contains("usrbin"));
}

/// `list --pretty` should leave the name column blank for unknown PATH segments.
#[test]
fn list_pretty_leaves_name_blank_for_unknown_segments() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "/some/unknown/path");
    let out = get_stdout(cmd.arg("list").arg("--pretty"));

    // The segment must appear.
    let seg_line = out
        .lines()
        .find(|l| l.contains("/some/unknown/path"))
        .unwrap();

    // After the path, the line should have only trailing whitespace (no name).
    assert!(
        seg_line.trim_end().ends_with("/some/unknown/path")
            || seg_line["/some/unknown/path".len()..].trim().is_empty()
    );
}

/// `list --pretty` with an empty PATH should print only header rows.
#[test]
fn list_pretty_with_empty_path_prints_only_table_header() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = test_cmd(dir, "");
    let out = get_stdout(cmd.arg("list").arg("--pretty"));

    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "PATH  NAME");
    assert_eq!(lines[1], "----  ----");
}
