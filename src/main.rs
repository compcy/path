//! Command-line utility for inspecting and manipulating shell PATH values.
//!
//! The binary supports adding, removing, deleting, and listing entries while
//! optionally persisting named mappings in a store file (default: `$HOME/.path`).
#![deny(warnings)]

use clap::{App, Arg, ArgMatches, SubCommand};
use regex::Regex;
use std::env;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// Simple representation of a stored path entry in plain text form.
#[derive(Debug, Clone)]
struct PathEntry {
    location: String,
    name: String,
    autoset: bool,
    line_number: usize,
}

const DEFAULT_STORE_FILE_NAME: &str = ".path";
const STORE_FILE_LAYOUT_COMMENT: &str = "# layout: <location> <name> <autoset?>";

/// Return the default store file path.
///
/// The default is `$HOME/.path`; if `HOME` is unavailable we fall back to a
/// relative `.path` in the current directory.
fn default_store_file_path() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(DEFAULT_STORE_FILE_NAME))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STORE_FILE_NAME))
}

/// Resolve the store file path from CLI arguments.
fn resolve_store_file_path(matches: &ArgMatches) -> PathBuf {
    matches
        .value_of("store_file")
        .map(PathBuf::from)
        .unwrap_or_else(default_store_file_path)
}

/// Parse a line from the store file.
///
/// Preferred format is `<location> <name> <autoset?>` with fields separated by
/// whitespace. To preserve embedded whitespace within fields, any whitespace
/// and `\\` characters are escaped with `\\` (for example `/tmp/my\ tools`).
///
/// `autoset` is `auto` or `noauto`; missing or blank autoset values are
/// treated as `auto`.
fn parse_autoset_value(value: &str) -> bool {
    match value {
        "" | "auto" => true,
        "noauto" => false,
        other => {
            eprintln!(
                "warning: invalid autoset value '{}', defaulting to auto",
                other
            );
            true
        }
    }
}

/// Split a whitespace-delimited line while honoring backslash escapes.
fn store_field_regex() -> &'static Regex {
    static FIELD_RE: OnceLock<Regex> = OnceLock::new();
    FIELD_RE.get_or_init(|| {
        Regex::new(r"(?:\\.|[^\s\\]|\\)+").expect("store field regex should be valid")
    })
}

/// Decode backslash escapes in a single store field.
fn unescape_store_field(field: &str) -> String {
    let mut decoded = String::with_capacity(field.len());
    let mut escaped = false;

    for character in field.chars() {
        if escaped {
            decoded.push(character);
            escaped = false;
            continue;
        }

        if character == '\\' {
            escaped = true;
            continue;
        }

        decoded.push(character);
    }

    if escaped {
        // Preserve a trailing backslash as a literal character.
        decoded.push('\\');
    }

    decoded
}

/// Split a whitespace-delimited line into escaped fields using a regex tokenizer.
fn split_escaped_whitespace_fields(line: &str) -> Vec<String> {
    store_field_regex()
        .find_iter(line)
        .map(|m| unescape_store_field(m.as_str()))
        .collect()
}

/// Escape store fields so they can be written as whitespace-delimited tokens.
fn escape_store_field(field: &str) -> String {
    let mut escaped = String::with_capacity(field.len());
    for character in field.chars() {
        if character == '\\' || character.is_whitespace() {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn parse_entry_line(line: &str, line_number: usize) -> Option<PathEntry> {
    let trimmed = line.trim();

    // skip blank lines entirely
    if trimmed.is_empty() {
        return None;
    }

    // allow comments in the store file (for example, the generated header)
    if trimmed.starts_with('#') {
        return None;
    }

    let parts = split_escaped_whitespace_fields(line);

    let entry = match parts.as_slice() {
        [location, name] => PathEntry {
            location: strip_trailing_slash(location),
            name: name.clone(),
            autoset: true,
            line_number,
        },
        [location, name, autoset] => PathEntry {
            location: strip_trailing_slash(location),
            name: name.clone(),
            autoset: parse_autoset_value(autoset),
            line_number,
        },
        [location] => {
            eprintln!(
                "warning: malformed entry at line {}: missing required name field",
                line_number
            );
            PathEntry {
                location: strip_trailing_slash(location),
                name: String::new(),
                autoset: true,
                line_number,
            }
        }
        _ => {
            eprintln!(
                "warning: malformed entry at line {}: expected 2-3 fields; escape spaces as '\\ '",
                line_number
            );
            PathEntry {
                location: strip_trailing_slash(trimmed),
                name: String::new(),
                autoset: true,
                line_number,
            }
        }
    };

    Some(entry)
}

/// Serialize an entry back into a line.
fn format_entry_line(entry: &PathEntry) -> String {
    let autoset = if entry.autoset { "auto" } else { "noauto" };
    format!(
        "{} {} {}",
        escape_store_field(&entry.location),
        entry.name,
        autoset
    )
}

/// Load entries from the store file; if the file doesn't exist return an empty
/// vector.
fn load_entries(store_file: &Path) -> io::Result<Vec<PathEntry>> {
    if !store_file.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(store_file)?;
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
fn save_entries(store_file: &Path, entries: &[PathEntry]) -> io::Result<()> {
    let mut data = String::new();
    data.push_str(STORE_FILE_LAYOUT_COMMENT);
    data.push('\n');
    for e in entries {
        data.push_str(&format_entry_line(e));
        data.push('\n');
    }
    fs::write(store_file, data)
}

/// Entry point for the `path` utility.
///
/// Parses command-line arguments, handles the `add` subcommand (recording
/// entries and emitting the modified PATH string), or otherwise prints the
/// current `PATH` environment variable.  The function is intentionally kept
/// small; helper routines above manage persistence to the configured store file.
/// Check the existing entries, reporting any whose `location` does not
/// currently exist.  Nameless/invalid/duplicate names are fatal errors;
/// missing locations are warnings only. Returns an I/O error only if
/// reading fails.
fn validate_entries(store_file: &Path) -> io::Result<()> {
    let entries = load_entries(store_file)?;

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
            store_file.display(),
            e.line_number,
            e.location
        );
        std::process::exit(1);
    }

    // Stored locations must be absolute and canonical-looking so command
    // handlers can avoid repeated canonicalization.
    if let Some(e) = entries
        .iter()
        .find(|e| !is_store_location_canonical_like(&e.location))
    {
        eprintln!(
            "error: invalid stored location '{}' at line {}: locations in {} must be absolute, canonical-looking, and must not contain ':'",
            e.location,
            e.line_number,
            store_file.display()
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
        .arg(
            Arg::with_name("store_file")
                .long("file")
                .value_name("FILE")
                .help("Store file to read/write (default: $HOME/.path)")
                .takes_value(true)
                .global(true),
        )
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
                .about("Delete a stored entry from the configured store file")
                .arg(
                    Arg::with_name("location")
                        .help("Location or stored name to delete from the configured store file")
                        .required(true)
                        .index(1),
                ),
        )
        .subcommand(
            SubCommand::with_name("list").about("List entries stored in the configured store file"),
        )
        .subcommand(
            SubCommand::with_name("load")
                .about("Add all auto entries from the configured store file to PATH"),
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

/// Remove trailing `/` characters while preserving root (`/`).
fn strip_trailing_slash(path: &str) -> String {
    let mut normalized = path.to_string();
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    normalized
}

/// Return whether a path-like value contains a PATH segment separator.
fn contains_path_separator(path: &str) -> bool {
    path.contains(':')
}

/// Normalize a path into an absolute, canonical-looking string.
///
/// This performs lexical normalization (removing `.` and resolving `..`)
/// without requiring the path to exist.
fn normalize_absolute_path(path: &Path) -> String {
    let mut normalized = PathBuf::new();
    // Track depth of Normal components so we never pop past the root.
    let mut depth: usize = 0;
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push("/"),
            Component::Normal(segment) => {
                normalized.push(segment);
                depth += 1;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                // Clamp at root: only pop a Normal segment, never the RootDir,
                // so an absolute input like `/a/../../x` yields `/x` not `x`.
                if depth > 0 {
                    normalized.pop();
                    depth -= 1;
                }
            }
            Component::Prefix(_) => {}
        }
    }

    let normalized_str = normalized.to_string_lossy().into_owned();
    if normalized_str.is_empty() {
        "/".to_string()
    } else {
        normalized_str
    }
}

/// Canonicalize CLI path arguments only when they are relative.
///
/// Absolute arguments are returned unchanged.
fn canonicalize_relative_cli_argument(path: &str) -> String {
    if path.starts_with('/') {
        return strip_trailing_slash(path);
    }

    match env::current_dir() {
        Ok(cwd) => strip_trailing_slash(&normalize_absolute_path(&cwd.join(path))),
        Err(_) => path.to_string(),
    }
}

/// Return whether a stored location is absolute and canonical-looking.
///
/// Canonical-looking means it is absolute, contains no `.`/`..` components,
/// has no duplicate separators, and has no trailing slash (except `/`).
fn is_store_location_canonical_like(location: &str) -> bool {
    if !location.starts_with('/') {
        return false;
    }

    if location != "/" && location.ends_with('/') {
        return false;
    }

    if location.contains("//") {
        return false;
    }

    if contains_path_separator(location) {
        return false;
    }

    Path::new(location)
        .components()
        .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
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
    let normalized_candidate = strip_trailing_slash(candidate);
    current
        .split(':')
        .any(|segment| strip_trailing_slash(segment) == normalized_candidate)
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
/// In addition to the resolved `location`, this may also remove the original
/// raw argument form when provided.
fn remove_from_path(current: &str, location: &str, raw_path_arg: Option<&str>) -> String {
    let normalized_location = strip_trailing_slash(location);
    let normalized_raw = raw_path_arg.map(strip_trailing_slash);

    current
        .split(':')
        .filter(|segment| {
            let normalized_segment = strip_trailing_slash(segment);

            normalized_segment != normalized_location
                && match raw_path_arg {
                    Some(_) => normalized_raw
                        .as_ref()
                        .map(|raw| normalized_segment != *raw)
                        .unwrap_or(true),
                    None => true,
                }
        })
        .collect::<Vec<_>>()
        .join(":")
}

/// Format a stored entry for human-readable list output.
fn format_list_entry(entry: &PathEntry) -> String {
    let autoset_marker = if entry.autoset { "auto" } else { "noauto" };
    if entry.name != entry.location {
        format!("{} ({}) [{}]", entry.location, entry.name, autoset_marker)
    } else {
        format!("{} [{}]", entry.location, autoset_marker)
    }
}

/// Handle the `add` subcommand.
///
/// This resolves named entries, validates path shape, optionally persists a
/// named mapping, and prints the resulting PATH export command.
fn handle_add(add_matches: &ArgMatches, store_file: &Path) {
    let mut location = add_matches.value_of("location").unwrap().to_string();
    let mut resolved_by_name = false;

    match load_entries(store_file) {
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

    if !resolved_by_name && contains_path_separator(&location) {
        eprintln!("error: path '{}' must not contain ':'", location);
        std::process::exit(1);
    }

    if !resolved_by_name {
        location = canonicalize_relative_cli_argument(&location);
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

        match load_entries(store_file) {
            Ok(mut entries) => {
                if entries.iter().any(|entry| entry.name == name) {
                    eprintln!("error: name '{}' is already in use", name);
                    std::process::exit(1);
                }

                if !is_store_location_canonical_like(&location) {
                    eprintln!(
                        "error: cannot store location '{}': stored paths must be absolute, canonical-looking, and must not contain ':'",
                        location
                    );
                    std::process::exit(1);
                }

                entries.push(PathEntry {
                    location: location.clone(),
                    name,
                    autoset,
                    line_number: 0,
                });

                if let Err(error) = save_entries(store_file, &entries) {
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

    let prepend = add_matches.is_present("pre");
    let current = env::var("PATH").unwrap_or_default();

    let should_add = !path_contains_segment(&current, &location);

    let updated = if should_add {
        compose_path(&current, &location, prepend)
    } else {
        current
    };
    println!("{}", format_export_path(&updated));
}

/// Handle the `remove` subcommand.
///
/// This resolves a stored name (or validates a raw path), removes matching
/// segments from PATH, and prints the resulting export command.
fn handle_remove(remove_matches: &ArgMatches, store_file: &Path) {
    let argument = remove_matches.value_of("location").unwrap().to_string();
    let mut location_to_remove = argument.clone();
    let mut resolved_by_name = false;

    if let Ok(entries) = load_entries(store_file) {
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

        if contains_path_separator(&argument) {
            eprintln!("error: path '{}' must not contain ':'", argument);
            std::process::exit(1);
        }

        location_to_remove = canonicalize_relative_cli_argument(&argument);
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
fn handle_delete(delete_matches: &ArgMatches, store_file: &Path) {
    let argument = delete_matches.value_of("location").unwrap().to_string();

    let mut entries = match load_entries(store_file) {
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

        if contains_path_separator(&argument) {
            eprintln!("error: path '{}' must not contain ':'", argument);
            std::process::exit(1);
        }

        let location_to_delete = canonicalize_relative_cli_argument(&argument);
        entries.retain(|entry| entry.location != location_to_delete);
    }

    if let Err(error) = save_entries(store_file, &entries) {
        eprintln!("warning: failed to update store file: {}", error);
    }
}

/// Handle the `list` subcommand by printing all stored entries.
fn handle_list(store_file: &Path) {
    let store_exists = store_file.exists();

    match load_entries(store_file) {
        Ok(entries) => {
            if entries.is_empty() {
                if store_exists {
                    println!("No stored entries.");
                } else {
                    println!("No stored entries: store file does not exist.");
                }
                return;
            }

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
/// This appends all entries marked `auto` in the configured store file to PATH, skipping
/// entries already present as exact path segments.
fn handle_load(store_file: &Path) {
    let entries = match load_entries(store_file) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("error: failed to load entries: {}", error);
            std::process::exit(1);
        }
    };

    let mut current = env::var("PATH").unwrap_or_default();
    for entry in entries.into_iter().filter(|entry| entry.autoset) {
        if !path_contains_segment(&current, &entry.location) {
            current = compose_path(&current, &entry.location, false);
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
    let matches = build_cli().get_matches();
    let store_file = resolve_store_file_path(&matches);

    if let Err(error) = validate_entries(&store_file) {
        eprintln!("warning: could not validate entries: {}", error);
    }

    if let Some(add_matches) = matches.subcommand_matches("add") {
        handle_add(add_matches, &store_file);
        return;
    }

    if let Some(remove_matches) = matches.subcommand_matches("remove") {
        handle_remove(remove_matches, &store_file);
        return;
    }

    if let Some(delete_matches) = matches.subcommand_matches("delete") {
        handle_delete(delete_matches, &store_file);
        return;
    }

    if matches.subcommand_matches("list").is_some() {
        handle_list(&store_file);
        return;
    }

    if matches.subcommand_matches("load").is_some() {
        handle_load(&store_file);
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
        let entry = parse_entry_line("/tmp/tools/ tools noauto", 7).unwrap();
        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "tools");
        assert!(!entry.autoset);
        assert_eq!(entry.line_number, 7);
    }

    #[test]
    /// Ensure older tab-delimited lines are still supported.
    fn parse_entry_line_legacy_tab_delimited_still_parses() {
        let entry = parse_entry_line("/tmp/tools\ttools\tnoauto", 8).unwrap();
        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "tools");
        assert!(!entry.autoset);
    }

    #[test]
    /// Verify lines without a name field become nameless entries for validation.
    fn parse_entry_line_without_name_field_creates_nameless_entry() {
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
        assert!(path_contains_segment("/a:/b/:/c", "/b"));
    }

    #[test]
    /// Ensure relative CLI paths are canonicalized once into absolute form.
    fn canonicalize_relative_cli_argument_resolves_dot() {
        let canonical = canonicalize_relative_cli_argument(".");
        let cwd = env::current_dir().unwrap().to_string_lossy().into_owned();
        assert_eq!(canonical, cwd);
    }

    #[test]
    /// Ensure absolute CLI paths are preserved as-is.
    fn canonicalize_relative_cli_argument_keeps_absolute() {
        assert_eq!(
            canonicalize_relative_cli_argument("/usr/local/bin"),
            "/usr/local/bin"
        );
    }

    #[test]
    /// Ensure `..` traversal beyond the root is clamped so the result stays absolute.
    fn normalize_absolute_path_clamps_at_root() {
        // `/a/../../x` would naively collapse to `x`; it must stay `/x`.
        assert_eq!(normalize_absolute_path(Path::new("/a/../../x")), "/x");
        // `/a/../..` must collapse to `/`, not an empty or relative path.
        assert_eq!(normalize_absolute_path(Path::new("/a/../..")), "/");
        // Normal `..` resolution within root should still work.
        assert_eq!(normalize_absolute_path(Path::new("/a/b/../c")), "/a/c");
    }

    #[test]
    /// Ensure canonical-looking validation rejects relative and non-normalized paths.
    fn is_store_location_canonical_like_validates_shape() {
        assert!(is_store_location_canonical_like("/usr/local/bin"));
        assert!(is_store_location_canonical_like("/"));

        assert!(!is_store_location_canonical_like("./rel"));
        assert!(!is_store_location_canonical_like("/usr:local/bin"));
        assert!(!is_store_location_canonical_like("/usr//local/bin"));
        assert!(!is_store_location_canonical_like("/usr/../local/bin"));
        assert!(!is_store_location_canonical_like("/usr/local/bin/"));
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
        assert_eq!(remove_from_path("/a:/b/:/c", "/b", None), "/a:/c");
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
    /// Ensure list formatting includes names when distinct and always includes autoset status.
    fn format_list_entry_includes_name_when_different() {
        let entry = PathEntry {
            location: "/usr/local/bin".to_string(),
            name: "tools".to_string(),
            autoset: true,
            line_number: 0,
        };
        assert_eq!(format_list_entry(&entry), "/usr/local/bin (tools) [auto]");

        let same = PathEntry {
            location: "/usr/bin".to_string(),
            name: "/usr/bin".to_string(),
            autoset: false,
            line_number: 0,
        };
        assert_eq!(format_list_entry(&same), "/usr/bin [noauto]");
    }

    #[test]
    /// Ensure missing third fields are interpreted as `auto` for manual edits.
    fn parse_entry_line_missing_third_field_defaults_to_auto() {
        let entry = parse_entry_line("/tmp/tools tools", 2).unwrap();
        assert!(entry.autoset);
    }

    #[test]
    /// Ensure escaped whitespace format preserves spaces in stored locations.
    fn parse_entry_line_escaped_whitespace_preserves_spaces_in_location() {
        let entry = parse_entry_line("/tmp/my\\ tools tools auto", 4).unwrap();
        assert_eq!(entry.location, "/tmp/my tools");
        assert_eq!(entry.name, "tools");
        assert!(entry.autoset);
    }

    #[test]
    /// Ensure escaped backslashes are decoded from store lines.
    fn parse_entry_line_decodes_escaped_backslash() {
        let entry = parse_entry_line("/tmp/my\\\\tools tools auto", 4).unwrap();
        assert_eq!(entry.location, "/tmp/my\\tools");
    }

    #[test]
    /// Ensure comment lines in the store file are ignored by the parser.
    fn parse_entry_line_comment_line_is_ignored() {
        assert!(parse_entry_line("# layout: <location> <name> <autoset?>", 1).is_none());
        assert!(parse_entry_line("   # user note", 2).is_none());
    }

    #[test]
    /// Ensure unescaped spaces create a malformed nameless entry.
    fn parse_entry_line_unescaped_spaced_location_is_malformed() {
        let entry = parse_entry_line("/tmp/my tools tools auto", 5).unwrap();
        assert_eq!(entry.location, "/tmp/my tools tools auto");
        assert_eq!(entry.name, "");
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
        assert_eq!(format_entry_line(&auto), "/tmp/a a auto");

        let noauto = PathEntry {
            location: "/tmp/b".to_string(),
            name: "b".to_string(),
            autoset: false,
            line_number: 0,
        };
        assert_eq!(format_entry_line(&noauto), "/tmp/b b noauto");
    }

    #[test]
    /// Ensure serializer escapes whitespace and backslashes in fields.
    fn format_entry_line_escapes_location_whitespace() {
        let spaced = PathEntry {
            location: "/tmp/my tools".to_string(),
            name: "tools".to_string(),
            autoset: true,
            line_number: 0,
        };
        assert_eq!(format_entry_line(&spaced), "/tmp/my\\ tools tools auto");

        let backslash = PathEntry {
            location: "/tmp/my\\tools".to_string(),
            name: "tools".to_string(),
            autoset: true,
            line_number: 0,
        };
        assert_eq!(format_entry_line(&backslash), "/tmp/my\\\\tools tools auto");
    }
}
