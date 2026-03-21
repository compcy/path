#![deny(warnings)]

// Integration tests exercising the command-line interface.  We
// deliberately run the compiled binary and manipulate the temporary
// working directory and environment variables instead of calling internal
// functions, as this ensures the CLI remains stable.
use assert_cmd::cargo;
use std::fs;
use std::path::Path;
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
    cmd.arg("add").arg("--pre").arg("/tmp/y").arg("yname");
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
    let store = dir.join(".path");
    fs::write(&store, "'/opt/auto' [a] (auto)\n'/opt/no' [n] (noauto)\n").unwrap();

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
    let store = dir.join(".path");
    // write one invalid and one valid line
    fs::write(
        &store,
        "'/no/such/thing' [bad] (auto)\n'/tmp' [tmp] (auto)\n",
    )
    .unwrap();

    let mut cmd = test_cmd(dir, "");
    cmd.arg("list");
    let assert = cmd.assert().success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("warning: the following stored paths do not exist"));
    assert!(stderr.contains("/no/such/thing"));

    // store file should remain unchanged
    let contents = fs::read_to_string(store).unwrap();
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
    let store = dir.join(".path");
    // write two entries with the same name "dup"
    fs::write(
        &store,
        "'/foo/a' [dup] (auto)\n'/foo/b' [dup] (auto)\n'/foo/c' [unique] (auto)\n",
    )
    .unwrap();

    let mut cmd = test_cmd(dir, "");
    let assert = cmd.arg("list").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error: duplicate name 'dup'"));
    assert!(stderr.contains("lines: 1, 2"));
    // file should remain untouched
    let contents = fs::read_to_string(store).unwrap();
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
    cmd.arg("add").arg("--pre").arg("/tmp/y").arg("yname");
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
    cmd.arg("add").arg("--pre").arg("/tmp/y").arg("yname");
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
    cmd.arg("add").arg("--pre").arg("/tmp/y").arg("yname");
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
        (
            "parentheses in location",
            "'/tmp/(evil)' [bad] (auto)\n",
            "error: invalid stored location '/tmp/(evil)'",
        ),
        (
            "asymmetric open parenthesis in location",
            "'/tmp/(evil' [bad] (auto)\n",
            "error: invalid stored location '/tmp/(evil'",
        ),
        (
            "asymmetric close parenthesis in location",
            "'/tmp/evil)' [bad] (auto)\n",
            "error: invalid stored location '/tmp/evil)'",
        ),
        (
            "braces in location",
            "'/tmp/{evil}' [bad] (auto)\n",
            "error: invalid stored location '/tmp/{evil}'",
        ),
        (
            "asymmetric open brace in location",
            "'/tmp/{evil' [bad] (auto)\n",
            "error: invalid stored location '/tmp/{evil'",
        ),
        (
            "asymmetric close brace in location",
            "'/tmp/evil}' [bad] (auto)\n",
            "error: invalid stored location '/tmp/evil}'",
        ),
        (
            "square brackets in location",
            "'/tmp/[evil]' [bad] (auto)\n",
            "error: invalid stored location '/tmp/[evil]'",
        ),
        (
            "asymmetric open bracket in location",
            "'/tmp/[evil' [bad] (auto)\n",
            "error: invalid stored location '/tmp/[evil'",
        ),
        (
            "asymmetric close bracket in location",
            "'/tmp/evil]' [bad] (auto)\n",
            "error: invalid stored location '/tmp/evil]'",
        ),
        (
            "escaped close bracket in location",
            "'/tmp/evil\\]' [bad] (auto)\n",
            "error: invalid stored location '/tmp/evil\\]'",
        ),
        (
            "escaped close parenthesis in location",
            "'/tmp/evil\\)' [bad] (auto)\n",
            "error: invalid stored location '/tmp/evil\\)'",
        ),
        (
            "escaped close brace in location",
            "'/tmp/evil\\}' [bad] (auto)\n",
            "error: invalid stored location '/tmp/evil\\}'",
        ),
        (
            "name contains open bracket",
            "'/tmp/safe' [bad[] (auto)\n",
            "error: invalid name 'bad['",
        ),
        (
            "name contains close bracket",
            "'/tmp/safe' [bad]] (auto)\n",
            "error: invalid name 'bad]'",
        ),
        (
            "name contains open parenthesis",
            "'/tmp/safe' [ba(d)] (auto)\n",
            "error: invalid name 'ba(d)'",
        ),
        (
            "name contains close parenthesis",
            "'/tmp/safe' [ba)d] (auto)\n",
            "error: invalid name 'ba)d'",
        ),
        (
            "name contains open brace",
            "'/tmp/safe' [ba{d}] (auto)\n",
            "error: invalid name 'ba{d}'",
        ),
        (
            "name contains close brace",
            "'/tmp/safe' [ba}d] (auto)\n",
            "error: invalid name 'ba}d'",
        ),
        (
            "name missing closing bracket",
            "'/tmp/safe' [safe (auto)\n",
            "error: found nameless entry",
        ),
        (
            "name missing opening bracket",
            "'/tmp/safe' safe] (auto)\n",
            "error: found nameless entry",
        ),
        (
            "name empty brackets",
            "'/tmp/safe' [] (auto)\n",
            "error: found nameless entry",
        ),
        (
            "options contain open bracket",
            "'/tmp/safe' [safe] (a[uto)\n",
            "error: found nameless entry",
        ),
        (
            "options contain close bracket",
            "'/tmp/safe' [safe] (a]uto)\n",
            "error: found nameless entry",
        ),
        (
            "options contain open parenthesis",
            "'/tmp/safe' [safe] (a(uto)\n",
            "error: found nameless entry",
        ),
        (
            "options contain close parenthesis",
            "'/tmp/safe' [safe] (a)uto)\n",
            "error: found nameless entry",
        ),
        (
            "options contain open brace",
            "'/tmp/safe' [safe] (a{uto)\n",
            "error: found nameless entry",
        ),
        (
            "options contain close brace",
            "'/tmp/safe' [safe] (a}uto)\n",
            "error: found nameless entry",
        ),
        (
            "options nested braces token",
            "'/tmp/safe' [safe] (auto,{pre})\n",
            "error: found nameless entry",
        ),
        (
            "options nested brackets token",
            "'/tmp/safe' [safe] (auto,[pre])\n",
            "error: found nameless entry",
        ),
        (
            "options nested parentheses token",
            "'/tmp/safe' [safe] (auto,(pre))\n",
            "error: found nameless entry",
        ),
        (
            "options missing closing parenthesis",
            "'/tmp/safe' [safe] (auto\n",
            "error: found nameless entry",
        ),
        (
            "options missing opening parenthesis",
            "'/tmp/safe' [safe] auto)\n",
            "error: found nameless entry",
        ),
        (
            "backtick in location",
            "'/tmp/`evil`' [bad] (auto)\n",
            "error: invalid stored location '/tmp/`evil`'",
        ),
        (
            "asymmetric backtick in location",
            "'/tmp/evil`' [bad] (auto)\n",
            "error: invalid stored location '/tmp/evil`'",
        ),
        (
            "name contains backtick",
            "'/tmp/safe' [ba`d] (auto)\n",
            "error: invalid name 'ba`d'",
        ),
        (
            "options contain backtick",
            "'/tmp/safe' [safe] (au`to)\n",
            "error: found nameless entry",
        ),
        (
            "semicolon in location",
            "'/tmp/ev;il' [bad] (auto)\n",
            "error: invalid stored location '/tmp/ev;il'",
        ),
        (
            "dollar sign in location",
            "'/tmp/$evil' [bad] (auto)\n",
            "error: invalid stored location '/tmp/$evil'",
        ),
        (
            "pipe in location",
            "'/tmp/ev|il' [bad] (auto)\n",
            "error: invalid stored location '/tmp/ev|il'",
        ),
        (
            "wildcard star in location",
            "'/tmp/ev*il' [bad] (auto)\n",
            "error: invalid stored location '/tmp/ev*il'",
        ),
        (
            "wildcard question mark in location",
            "'/tmp/ev?il' [bad] (auto)\n",
            "error: invalid stored location '/tmp/ev?il'",
        ),
        (
            "ampersand in location",
            "'/tmp/ev&il' [bad] (auto)\n",
            "error: invalid stored location '/tmp/ev&il'",
        ),
        (
            "redirect less-than in location",
            "'/tmp/ev<il' [bad] (auto)\n",
            "error: invalid stored location '/tmp/ev<il'",
        ),
        (
            "redirect greater-than in location",
            "'/tmp/ev>il' [bad] (auto)\n",
            "error: invalid stored location '/tmp/ev>il'",
        ),
        (
            "hash in location",
            "'/tmp/ev#il' [bad] (auto)\n",
            "error: invalid stored location '/tmp/ev#il'",
        ),
    ];

    for (label, line, expected_stderr) in cases {
        let temp = tempdir().unwrap();
        let dir = temp.path();
        let store = dir.join(".path");
        fs::write(&store, line).unwrap();

        let mut cmd = test_cmd(dir, "");
        let assert = cmd.arg("list").assert().failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        assert!(
            stderr.contains(expected_stderr),
            "case '{}' expected stderr to contain '{}', got: {}",
            label,
            expected_stderr,
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
