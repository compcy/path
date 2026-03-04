//! Command-line utility for inspecting and manipulating shell PATH values.
//!
//! The binary supports adding, removing, deleting, and listing entries while
//! optionally persisting named mappings in a local `.path` store file.
#![deny(warnings)]

use clap::{App, Arg, ArgMatches, SubCommand};
use std::env;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

/// Simple representation of a stored path entry in plain text form.
#[derive(Debug, Clone)]
struct PathEntry {
    location: String,
    name: String,
    autoset: bool,
    line_number: usize,
}

const STORE_FILE: &str = ".path";

/// Parse a line from the store file.
///
/// Format is `<location>\t<name>\t<autoset?>` where autoset is `auto` or
/// `noauto`. Missing or blank autoset values are treated as `auto`.
fn parse_entry_line(line: &str, line_number: usize) -> Option<PathEntry> {
    // skip blank lines entirely
    if line.trim().is_empty() {
        return None;
    }

    // split on tabs; a well-formed line has at least two fields (location
    // and name).  In earlier versions we silently dropped lines lacking the
    // second field, which meant a user could have a malformed `.path` file
    // and our validation logic would never see it.  Instead we now treat the
    // missing name case as an entry with an empty name so that
    // `validate_entries` will catch it and abort.
    let parts: Vec<&str> = line.splitn(3, '\t').collect();
    let location = parts[0].to_string();
    let name = if parts.len() >= 2 {
        parts[1].to_string()
    } else {
        // no tab found; this is effectively a nameless entry
        String::new()
    };

    let autoset = if parts.len() == 3 {
        match parts[2].trim() {
            "auto" | "" => true,
            "noauto" => false,
            other => {
                // ignore malformed value
                eprintln!(
                    "warning: invalid autoset value '{}', defaulting to auto",
                    other
                );
                true
            }
        }
    } else {
        true
    };
    Some(PathEntry {
        location,
        name,
        autoset,
        line_number,
    })
}

/// Serialize an entry back into a line.
fn format_entry_line(entry: &PathEntry) -> String {
    let autoset = if entry.autoset { "auto" } else { "noauto" };
    format!("{}\t{}\t{}", entry.location, entry.name, autoset)
}

/// Load entries from the STORE_FILE; if the file doesn't exist return an empty
/// vector.
fn load_entries() -> io::Result<Vec<PathEntry>> {
    let path = Path::new(STORE_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut entries = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if let Some(e) = parse_entry_line(&line, index + 1) {
            entries.push(e);
        }
    }
    Ok(entries)
}

/// Validate that a name contains only alphanumeric characters.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric())
}

/// Write all provided entries back to the store file, overwriting it.
fn save_entries(entries: &[PathEntry]) -> io::Result<()> {
    let mut data = String::new();
    for e in entries {
        data.push_str(&format_entry_line(e));
        data.push('\n');
    }
    fs::write(STORE_FILE, data)
}

/// Entry point for the `path` utility.
///
/// Parses command-line arguments, handles the `add` subcommand (recording
/// entries and emitting the modified PATH string), or otherwise prints the
/// current `PATH` environment variable.  The function is intentionally kept
/// small; helper routines above manage persistence to the `.path` store.
/// Check the existing entries, reporting any whose `location` does not
/// currently exist.  Nameless/invalid/duplicate names are fatal errors;
/// missing locations are warnings only. Returns an I/O error only if
/// reading fails.
fn validate_entries() -> io::Result<()> {
    let entries = load_entries()?;

    // Ensure there are no entries without a name.  If one is discovered we
    // treat this as a fatal error: the user should correct or remove the
    // offending line manually.  We print the location so they know which line
    // to fix and then exit with non-zero status.
    // If we encounter any stored entry without a name, that's a fatal
    // configuration error.  Clippy warns about a loop that always exits, so
    // we use `find` instead of iterating explicitly.
    if let Some(e) = entries.iter().find(|e| e.name.is_empty()) {
        eprintln!(
            "error: found nameless entry in {} at line {}: '{}'",
            STORE_FILE, e.line_number, e.location
        );
        std::process::exit(1);
    }

    // Check for invalid names (must be alphanumeric)
    if let Some(e) = entries.iter().find(|e| !is_valid_name(&e.name)) {
        eprintln!(
            "error: invalid name '{}' at line {}: names must contain only alphanumeric characters",
            e.name, e.line_number
        );
        std::process::exit(1);
    }

    // Check for duplicate names and report all occurrences with their line
    // numbers so the user can resolve the conflict.
    let mut seen_names = std::collections::HashMap::new();
    for e in entries.iter() {
        seen_names
            .entry(&e.name)
            .or_insert_with(Vec::new)
            .push(e.line_number);
    }
    let duplicates: Vec<_> = seen_names
        .iter()
        .filter(|(_, lines)| lines.len() > 1)
        .collect();
    if !duplicates.is_empty() {
        for (name, lines) in duplicates {
            let line_list = lines
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "error: duplicate name '{}' found at lines: {}",
                name, line_list
            );
        }
        std::process::exit(1);
    }

    // Warn about paths that don't exist, but do not delete them automatically
    let invalid: Vec<&PathEntry> = entries
        .iter()
        .filter(|e| !Path::new(&e.location).exists())
        .collect();
    if invalid.is_empty() {
        return Ok(());
    }

    eprintln!("warning: the following stored paths do not exist:");
    for e in invalid {
        eprintln!("  {}", e.location);
    }
    Ok(())
}

/// Build the command-line interface definition used by `clap`.
fn build_cli() -> App<'static, 'static> {
    App::new("path")
        .version("0.1.0")
        .about("Utility for inspecting and manipulating the PATH environment variable")
        .subcommand(
            SubCommand::with_name("add")
                .about("Add a directory to the PATH")
                .arg(
                    Arg::with_name("location")
                        .help("Location to add to PATH")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::with_name("name")
                        .help("Optional short name for this entry")
                        .required(false)
                        .index(2),
                )
                .arg(
                    Arg::with_name("noauto")
                        .long("noauto")
                        .help("Store this entry as not auto-loaded by 'path load'")
                        .takes_value(false),
                )
                .arg(
                    Arg::with_name("pre")
                        .long("pre")
                        .help("Prepend the location instead of appending")
                        .takes_value(false),
                ),
        )
        .subcommand(
            SubCommand::with_name("remove")
                .about("Remove a directory from PATH")
                .arg(
                    Arg::with_name("location")
                        .help("Location or stored name to remove from PATH")
                        .required(true)
                        .index(1),
                ),
        )
        .subcommand(
            SubCommand::with_name("delete")
                .about("Delete a stored entry from the .path file")
                .arg(
                    Arg::with_name("location")
                        .help("Location or stored name to delete from .path")
                        .required(true)
                        .index(1),
                ),
        )
        .subcommand(SubCommand::with_name("list").about("List entries stored in the .path file"))
        .subcommand(
            SubCommand::with_name("load").about("Add all auto entries from the .path file to PATH"),
        )
}

/// Resolve a user-provided token to a stored location when it matches a name.
fn resolve_location_by_name(input: &str, entries: &[PathEntry]) -> Option<String> {
    entries
        .iter()
        .find(|entry| entry.name == input)
        .map(|entry| entry.location.clone())
}

/// Return whether a raw path argument uses an allowed form.
///
/// Supported forms are absolute paths and relative paths that begin with `.`.
fn is_path_argument_valid(path: &str) -> bool {
    path.starts_with('/') || path.starts_with('.')
}

/// Compose a new PATH string by prepending or appending a location.
fn compose_path(current: &str, location: &str, prepend: bool) -> String {
    if current.is_empty() {
        location.to_string()
    } else if prepend {
        format!("{}:{}", location, current)
    } else {
        format!("{}:{}", current, location)
    }
}

/// Check whether a PATH-like string already contains an exact segment match.
fn path_contains_segment(current: &str, candidate: &str) -> bool {
    current.split(':').any(|segment| segment == candidate)
}

/// Canonicalize a location for comparison or output in PATH updates.
///
/// Returns `None` when canonicalization fails.
fn canonicalize_for_path_output(location: &str) -> Option<String> {
    fs::canonicalize(location)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

/// Determine whether PATH already contains a directory equivalent to `candidate`.
///
/// This checks both exact string matches and canonicalized filesystem
/// equivalence to avoid duplicate entries that differ only syntactically.
fn path_contains_equivalent_directory(current: &str, candidate: &str) -> bool {
    if path_contains_segment(current, candidate) {
        return true;
    }

    let candidate_canonical = match canonicalize_for_path_output(candidate) {
        Some(path) => path,
        None => return false,
    };

    current.split(':').any(|segment| {
        canonicalize_for_path_output(segment)
            .map(|canonical| canonical == candidate_canonical)
            .unwrap_or(false)
    })
}

/// Escape single quotes for safe embedding inside a single-quoted shell string.
fn quote_for_shell_single(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// Format a shell export statement for the provided PATH value.
fn format_export_path(path: &str) -> String {
    format!("export PATH='{}'", quote_for_shell_single(path))
}

/// Remove matching path segments from a PATH-like string.
///
/// In addition to the canonical `location`, this may also remove the original
/// raw argument form when provided.
fn remove_from_path(current: &str, location: &str, raw_path_arg: Option<&str>) -> String {
    current
        .split(':')
        .filter(|segment| {
            *segment != location
                && match raw_path_arg {
                    Some(raw) => *segment != raw,
                    None => true,
                }
        })
        .collect::<Vec<_>>()
        .join(":")
}

/// Format a stored entry for human-readable list output.
fn format_list_entry(entry: &PathEntry) -> String {
    if entry.name != entry.location {
        format!("{} ({})", entry.location, entry.name)
    } else {
        entry.location.clone()
    }
}

/// Handle the `add` subcommand.
///
/// This resolves named entries, validates path shape, optionally persists a
/// named mapping, and prints the resulting PATH export command.
fn handle_add(add_matches: &ArgMatches) {
    let mut location = add_matches.value_of("location").unwrap().to_string();
    let mut resolved_by_name = false;

    match load_entries() {
        Ok(entries) => {
            if let Some(resolved_location) = resolve_location_by_name(&location, &entries) {
                location = resolved_location;
                resolved_by_name = true;
            }
        }
        Err(error) => {
            eprintln!(
                "warning: failed to load store file, treating argument as path: {}",
                error
            );
        }
    }

    if !resolved_by_name && !is_path_argument_valid(&location) {
        eprintln!(
            "error: path '{}' must be absolute or start with '.'",
            location
        );
        std::process::exit(1);
    }

    if Path::new(&location).exists() {
        match fs::metadata(&location) {
            Ok(meta) if !meta.is_dir() => {
                eprintln!("error: '{}' exists but is not a directory", location);
                std::process::exit(1);
            }
            _ => {}
        }
    } else {
        eprintln!("warning: added path '{}' does not exist", location);
    }

    let name_opt = add_matches.value_of("name").map(|value| value.to_string());
    let autoset = !add_matches.is_present("noauto");

    if let Some(name) = name_opt {
        if !is_valid_name(&name) {
            eprintln!(
                "error: invalid name '{}': names must contain only alphanumeric characters",
                name
            );
            std::process::exit(1);
        }

        match load_entries() {
            Ok(mut entries) => {
                if entries.iter().any(|entry| entry.name == name) {
                    eprintln!("error: name '{}' is already in use", name);
                    std::process::exit(1);
                }

                let stored_location = match fs::canonicalize(&location) {
                    Ok(path) => path.to_string_lossy().into_owned(),
                    Err(_) => {
                        eprintln!(
                            "warning: could not canonicalize path '{}', storing as-is",
                            location
                        );
                        location.clone()
                    }
                };

                entries.push(PathEntry {
                    location: stored_location,
                    name,
                    autoset,
                    line_number: 0,
                });

                if let Err(error) = save_entries(&entries) {
                    eprintln!("warning: failed to update store file: {}", error);
                }
            }
            Err(error) => {
                eprintln!(
                    "warning: failed to load store file, not updating named entries: {}",
                    error
                );
            }
        }
    }

    let env_location = canonicalize_for_path_output(&location).unwrap_or_else(|| location.clone());
    let prepend = add_matches.is_present("pre");
    let current = env::var("PATH").unwrap_or_default();

    let should_add = !path_contains_equivalent_directory(&current, &env_location);

    let updated = if should_add {
        compose_path(&current, &env_location, prepend)
    } else {
        current
    };
    println!("{}", format_export_path(&updated));
}

/// Handle the `remove` subcommand.
///
/// This resolves a stored name (or validates a raw path), removes matching
/// segments from PATH, and prints the resulting export command.
fn handle_remove(remove_matches: &ArgMatches) {
    let argument = remove_matches.value_of("location").unwrap().to_string();
    let mut location_to_remove = argument.clone();
    let mut resolved_by_name = false;

    if let Ok(entries) = load_entries() {
        if let Some(resolved_location) = resolve_location_by_name(&argument, &entries) {
            location_to_remove = resolved_location;
            resolved_by_name = true;
        }
    }

    let mut raw_path_arg = None;
    if !resolved_by_name {
        if !is_path_argument_valid(&argument) {
            eprintln!(
                "error: path '{}' must be absolute or start with '.'",
                argument
            );
            std::process::exit(1);
        }

        location_to_remove = match fs::canonicalize(&argument) {
            Ok(path) => path.to_string_lossy().into_owned(),
            Err(_) => argument.clone(),
        };
        raw_path_arg = Some(argument.as_str());
    }

    let current = env::var("PATH").unwrap_or_default();
    println!(
        "{}",
        format_export_path(&remove_from_path(
            &current,
            &location_to_remove,
            raw_path_arg
        ))
    );
}

/// Handle the `delete` subcommand.
///
/// This updates the on-disk store by removing an entry by name or by location.
fn handle_delete(delete_matches: &ArgMatches) {
    let argument = delete_matches.value_of("location").unwrap().to_string();

    let mut entries = match load_entries() {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("error: failed to load entries: {}", error);
            std::process::exit(1);
        }
    };

    if let Some(position) = entries.iter().position(|entry| entry.name == argument) {
        entries.remove(position);
    } else {
        if !is_path_argument_valid(&argument) {
            eprintln!(
                "error: path '{}' must be absolute or start with '.'",
                argument
            );
            std::process::exit(1);
        }

        let location_to_delete = match fs::canonicalize(&argument) {
            Ok(path) => path.to_string_lossy().into_owned(),
            Err(_) => argument.clone(),
        };
        entries.retain(|entry| entry.location != location_to_delete);
    }

    if let Err(error) = save_entries(&entries) {
        eprintln!("warning: failed to update store file: {}", error);
    }
}

/// Handle the `list` subcommand by printing all stored entries.
fn handle_list() {
    match load_entries() {
        Ok(entries) => {
            for entry in entries {
                println!("{}", format_list_entry(&entry));
            }
        }
        Err(error) => {
            eprintln!("error: failed to load entries: {}", error);
            std::process::exit(1);
        }
    }
}

/// Handle the `load` subcommand.
///
/// This appends all entries marked `auto` in `.path` to PATH, skipping
/// directories that are already present (including canonical equivalents).
fn handle_load() {
    let entries = match load_entries() {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("error: failed to load entries: {}", error);
            std::process::exit(1);
        }
    };

    let mut current = env::var("PATH").unwrap_or_default();
    for entry in entries.into_iter().filter(|entry| entry.autoset) {
        let env_location =
            canonicalize_for_path_output(&entry.location).unwrap_or_else(|| entry.location.clone());
        if !path_contains_equivalent_directory(&current, &env_location) {
            current = compose_path(&current, &env_location, false);
        }
    }

    println!("{}", format_export_path(&current));
}

/// Print the current PATH value as a shell export statement.
fn print_current_path() {
    match env::var("PATH") {
        Ok(path) => println!("{}", format_export_path(&path)),
        Err(error) => eprintln!("Failed to read PATH: {}", error),
    }
}

/// Program entry point.
///
/// Validates stored entries, dispatches subcommands, and falls back to
/// printing the current PATH when no subcommand is provided.
fn main() {
    if let Err(error) = validate_entries() {
        eprintln!("warning: could not validate entries: {}", error);
    }

    let matches = build_cli().get_matches();

    if let Some(add_matches) = matches.subcommand_matches("add") {
        handle_add(add_matches);
        return;
    }

    if let Some(remove_matches) = matches.subcommand_matches("remove") {
        handle_remove(remove_matches);
        return;
    }

    if let Some(delete_matches) = matches.subcommand_matches("delete") {
        handle_delete(delete_matches);
        return;
    }

    if matches.subcommand_matches("list").is_some() {
        handle_list();
        return;
    }

    if matches.subcommand_matches("load").is_some() {
        handle_load();
        return;
    }

    print_current_path();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Verify three-field lines are parsed into all struct fields.
    fn parse_entry_line_parses_three_fields() {
        let entry = parse_entry_line("/tmp/tools\ttools\tnoauto", 7).unwrap();
        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "tools");
        assert!(!entry.autoset);
        assert_eq!(entry.line_number, 7);
    }

    #[test]
    /// Verify lines without a tab become nameless entries for later validation.
    fn parse_entry_line_without_tab_creates_nameless_entry() {
        let entry = parse_entry_line("/tmp/tools", 3).unwrap();
        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "");
        assert!(entry.autoset);
    }

    #[test]
    /// Ensure path composition handles both prepend and append behavior.
    fn compose_path_appends_or_prepends() {
        assert_eq!(compose_path("A:B", "/tmp/x", false), "A:B:/tmp/x");
        assert_eq!(compose_path("A:B", "/tmp/x", true), "/tmp/x:A:B");
        assert_eq!(compose_path("", "/tmp/x", false), "/tmp/x");
        assert_eq!(compose_path("", "/tmp/x", true), "/tmp/x");
    }

    #[test]
    /// Confirm exact-segment matching in colon-delimited PATH strings.
    fn path_contains_segment_matches_exact_entries() {
        assert!(path_contains_segment("/a:/b:/c", "/b"));
        assert!(!path_contains_segment("/a:/b:/c", "/d"));
    }

    #[test]
    /// Confirm canonical-equivalent directories are treated as already present.
    fn path_contains_equivalent_directory_matches_canonical_equivalent() {
        let cwd = env::current_dir().unwrap().to_string_lossy().into_owned();
        assert!(path_contains_equivalent_directory(".:/usr/bin", &cwd));
    }

    #[test]
    /// Ensure canonicalization resolves `.` to the current working directory.
    fn canonicalize_for_path_output_resolves_dot() {
        let canonical = canonicalize_for_path_output(".").unwrap();
        let cwd = env::current_dir().unwrap().to_string_lossy().into_owned();
        assert_eq!(canonical, cwd);
    }

    #[test]
    /// Ensure PATH export formatting includes shell-safe single-quote escaping.
    fn format_export_path_quotes_for_shell() {
        assert_eq!(format_export_path("/a:/b"), "export PATH='/a:/b'");
        assert_eq!(
            format_export_path("/a'quoted:/b"),
            "export PATH='/a'\\''quoted:/b'"
        );
    }

    #[test]
    /// Ensure remove logic drops all exact matching segments.
    fn remove_from_path_removes_exact_segments() {
        assert_eq!(remove_from_path("/a:/b:/c", "/b", None), "/a:/c");
        assert_eq!(remove_from_path("/a:/b:/a", "/a", None), "/b");
    }

    #[test]
    /// Ensure remove logic can also match the original raw argument form.
    fn remove_from_path_also_matches_raw_argument() {
        assert_eq!(
            remove_from_path("./rel:/usr/bin", "/abs/rel", Some("./rel")),
            "/usr/bin"
        );
    }

    #[test]
    /// Verify stored name lookup returns the expected location when present.
    fn resolve_location_by_name_returns_matching_location() {
        let entries = vec![PathEntry {
            location: "/usr/local/bin".to_string(),
            name: "tools".to_string(),
            autoset: true,
            line_number: 1,
        }];
        assert_eq!(
            resolve_location_by_name("tools", &entries),
            Some("/usr/local/bin".to_string())
        );
        assert_eq!(resolve_location_by_name("missing", &entries), None);
    }

    #[test]
    /// Ensure list formatting includes names only when distinct from location.
    fn format_list_entry_includes_name_when_different() {
        let entry = PathEntry {
            location: "/usr/local/bin".to_string(),
            name: "tools".to_string(),
            autoset: true,
            line_number: 0,
        };
        assert_eq!(format_list_entry(&entry), "/usr/local/bin (tools)");

        let same = PathEntry {
            location: "/usr/bin".to_string(),
            name: "/usr/bin".to_string(),
            autoset: true,
            line_number: 0,
        };
        assert_eq!(format_list_entry(&same), "/usr/bin");
    }

    #[test]
    /// Ensure blank third fields are interpreted as `auto` for manual edits.
    fn parse_entry_line_blank_third_field_defaults_to_auto() {
        let entry = parse_entry_line("/tmp/tools\ttools\t", 2).unwrap();
        assert!(entry.autoset);
    }

    #[test]
    /// Ensure entry serialization writes explicit `auto` and `noauto` markers.
    fn format_entry_line_writes_auto_markers() {
        let auto = PathEntry {
            location: "/tmp/a".to_string(),
            name: "a".to_string(),
            autoset: true,
            line_number: 0,
        };
        assert_eq!(format_entry_line(&auto), "/tmp/a\ta\tauto");

        let noauto = PathEntry {
            location: "/tmp/b".to_string(),
            name: "b".to_string(),
            autoset: false,
            line_number: 0,
        };
        assert_eq!(format_entry_line(&noauto), "/tmp/b\tb\tnoauto");
    }
}
