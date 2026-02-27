use assert_cmd::Command;
use predicates::prelude::*;
use std::env;
use tempfile::tempdir;
use std::fs;

#[test]
fn prints_path_env() {
    let mut cmd = Command::cargo_bin("path").unwrap();
    cmd.env("PATH", "foo:bar");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("foo:bar"));
}

#[test]
fn add_appends_and_records_entry() {
    let temp = tempdir().unwrap();
    let dir = temp.path();

    let mut cmd = Command::cargo_bin("path").unwrap();
    cmd.current_dir(&dir);
    cmd.env("PATH", "A");
    cmd.arg("add").arg("/tmp/x");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("A:/tmp/x"));

    // verify store file
    let store = dir.join(".path");
    let contents = fs::read_to_string(store).unwrap();
    assert!(contents.contains("/tmp/x\t/tmp/x"));
}

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
