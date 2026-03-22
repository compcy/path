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
    prepend: bool,
    protect: bool,
    invalid_option: Option<String>,
    original_options: Option<String>,
    line_number: usize,
}

impl PathEntry {
    // Create an entry with the default option and metadata state.
    fn new(location: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            name: name.into(),
            autoset: true,
            prepend: false,
            protect: false,
            invalid_option: None,
            original_options: None,
            line_number: 0,
        }
    }

    // Apply parsed or CLI-provided option flags without touching metadata.
    fn with_options(mut self, autoset: bool, prepend: bool, protect: bool) -> Self {
        self.autoset = autoset;
        self.prepend = prepend;
        self.protect = protect;
        self
    }

    // Preserve source line-number metadata for diagnostics.
    fn with_line_number(mut self, line_number: usize) -> Self {
        self.line_number = line_number;
        self
    }

    // Preserve the raw options token for unknown-option round-trip output.
    fn with_original_options(mut self, options_token: impl Into<String>) -> Self {
        self.original_options = Some(options_token.into());
        self
    }
}

const DEFAULT_STORE_FILE_NAME: &str = ".path";
const STORE_FILE_LAYOUT_COMMENT: &str = "# layout: '<location>' [<name>] (<options>)";

/// Build a built-in path entry as a `PathEntry`.
fn builtin_path_entry(location: &str, name: &str, prepend: bool, protect: bool) -> PathEntry {
    PathEntry::new(location, name).with_options(true, prepend, protect)
}

/// Standard system paths managed by `path restore`.
fn standard_system_paths() -> &'static [PathEntry] {
    static SYSTEM_PATHS: OnceLock<Vec<PathEntry>> = OnceLock::new();
    SYSTEM_PATHS
        .get_or_init(|| {
            vec![
                builtin_path_entry("/bin", "sysbin", false, true),
                builtin_path_entry("/sbin", "syssbin", false, true),
                builtin_path_entry("/usr/bin", "usrbin", false, true),
                builtin_path_entry("/usr/sbin", "usrsbin", false, true),
                builtin_path_entry("/usr/local/bin", "usrlocalbin", false, true),
                builtin_path_entry("/usr/local/sbin", "usrlocalsbin", false, true),
            ]
        })
        .as_slice()
}

/// Return the built-in protected system entry matching a reserved name.
fn find_system_path_by_name(name: &str) -> Option<&'static PathEntry> {
    standard_system_paths()
        .iter()
        .find(|entry| entry.name == name)
}

/// Return the built-in protected system entry matching a location.
fn find_system_path_by_location(location: &str) -> Option<&'static PathEntry> {
    standard_system_paths()
        .iter()
        .find(|entry| strip_trailing_slash(&entry.location) == strip_trailing_slash(location))
}

/// Known non-system tool paths recognised for display by `list --pretty`.
///
/// These entries are unprotected and are not managed by `path restore`. The
/// `$HOME`-relative entries are expanded from the current environment at first call,
/// then cached for efficient reuse across path segment processing.
fn known_extra_paths() -> &'static [PathEntry] {
    static EXTRA_PATHS: OnceLock<Vec<PathEntry>> = OnceLock::new();
    EXTRA_PATHS
        .get_or_init(|| {
            let mut entries = vec![
                builtin_path_entry("/opt/homebrew/bin", "homebrewbin", false, false),
                builtin_path_entry("/opt/homebrew/sbin", "homebrewsbin", false, false),
            ];

            if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
                let home_str = strip_trailing_slash(&home.to_string_lossy());
                entries.push(builtin_path_entry(
                    &format!("{}/.cargo/bin", home_str),
                    "cargo",
                    false,
                    false,
                ));
                entries.push(builtin_path_entry(
                    &format!("{}/.local/bin", home_str),
                    "pipx",
                    false,
                    false,
                ));
            }

            entries
        })
        .as_slice()
}

/// Return the known extra path entry matching a location, if any.
fn find_extra_path_by_location(location: &str) -> Option<PathEntry> {
    known_extra_paths()
        .iter()
        .find(|entry| strip_trailing_slash(&entry.location) == strip_trailing_slash(location))
        .cloned()
}

/// Return `true` if any name appears in both `PathEntry` slices.
#[cfg(test)]
fn path_entry_name_overlap(a: &[PathEntry], b: &[PathEntry]) -> bool {
    a.iter()
        .any(|left| b.iter().any(|right| left.name == right.name))
}

/// Return `true` if a `PathEntry` slice contains any duplicate names.
#[cfg(test)]
fn path_entries_have_unique_names(entries: &[PathEntry]) -> bool {
    entries.iter().enumerate().all(|(index, entry)| {
        entries
            .iter()
            .skip(index + 1)
            .all(|other| entry.name != other.name)
    })
}

/// Return whether all built-in path names are unique across system and extra
/// path tables.
#[cfg(test)]
fn builtins_have_unique_names() -> bool {
    let extra_paths = known_extra_paths();
    path_entries_have_unique_names(standard_system_paths())
        && path_entries_have_unique_names(extra_paths)
        && !path_entry_name_overlap(standard_system_paths(), extra_paths)
}

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
/// Preferred format is `'<location>' [<name>] (<options>)` with fields separated
/// by whitespace. Locations are wrapped in single quotes to make delimiter
/// boundaries explicit.
///
/// `name` is wrapped in `[]` and options are wrapped in `()`.
///
/// `autoset` options are `auto` or `noauto`; placement options are `pre` or
/// `post`; protection options include `protect`. Missing or blank options
/// default to `auto` plus post and no protection.
fn parse_autoset_value(value: &str) -> Option<bool> {
    match value {
        "auto" => Some(true),
        "noauto" => Some(false),
        _ => None,
    }
}

/// Strip a leading and trailing single quote from a field.
fn strip_wrapped(value: &str, open: char, close: char) -> Option<&str> {
    value
        .strip_prefix(open)
        .and_then(|inner| inner.strip_suffix(close))
}

/// Strip a leading and trailing single quote from a field.
fn strip_single_quotes(value: &str) -> Option<&str> {
    strip_wrapped(value, '\'', '\'')
}

/// Decode a stored name field.
fn parse_name_field(value: &str) -> Option<String> {
    strip_wrapped(value, '[', ']').map(|name| name.to_string())
}

/// Decode entry options from the third field.
///
/// Result for decoding a store entry options field.
enum EntryOptionsParseResult {
    Valid {
        autoset: bool,
        prepend: bool,
        protect: bool,
        invalid_option: Option<String>,
    },
    Malformed,
}

/// Supported options include autoset (`auto`/`noauto`), placement
/// (`pre`/`post`), and protection (`protect`). Options may be comma-delimited
/// and wrapped in `()`. Unknown alphabetic option tokens are returned to the
/// caller for diagnostics while recognized options still take effect;
/// malformed option shapes are rejected.
fn parse_entry_options(value: &str) -> EntryOptionsParseResult {
    let normalized = match strip_wrapped(value, '(', ')') {
        Some(raw) => raw.trim(),
        None => return EntryOptionsParseResult::Malformed,
    };

    if normalized.is_empty() {
        return EntryOptionsParseResult::Valid {
            autoset: true,
            prepend: false,
            protect: false,
            invalid_option: None,
        };
    }

    let mut autoset = true;
    let mut prepend = false;
    let mut protect = false;
    let mut invalid_option = None;

    for option in normalized
        .split(',')
        .map(str::trim)
        .filter(|opt| !opt.is_empty())
    {
        if let Some(parsed_autoset) = parse_autoset_value(option) {
            autoset = parsed_autoset;
            continue;
        }

        match option {
            "pre" => prepend = true,
            "post" => prepend = false,
            "protect" => protect = true,
            other => {
                if other
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
                {
                    if invalid_option.is_none() {
                        invalid_option = Some(other.to_string());
                    }
                    continue;
                }
                return EntryOptionsParseResult::Malformed;
            }
        }
    }

    EntryOptionsParseResult::Valid {
        autoset,
        prepend,
        protect,
        invalid_option,
    }
}

/// Render the option marker list for a stored entry.
fn format_entry_options(entry: &PathEntry) -> String {
    let autoset = if entry.autoset { "auto" } else { "noauto" };
    let mut options = vec![autoset.to_string()];

    if entry.prepend {
        options.push("pre".to_string());
    }

    if entry.protect {
        options.push("protect".to_string());
    }

    options.join(",")
}

/// Split a store line into fields while keeping a quoted location token intact.
fn store_field_regex() -> &'static Regex {
    static FIELD_RE: OnceLock<Regex> = OnceLock::new();
    FIELD_RE.get_or_init(|| Regex::new(r"'[^']*'|\S+").expect("store field regex should be valid"))
}

/// Split a store line into raw fields using a regex tokenizer.
fn split_store_fields(line: &str) -> Vec<String> {
    store_field_regex()
        .find_iter(line)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// Wrap a stored location field in single quotes.
fn quote_store_location_field(field: &str) -> String {
    format!("'{}'", field)
}

/// Build a malformed nameless entry with default option values.
fn malformed_nameless_entry(location: &str, line_number: usize) -> PathEntry {
    PathEntry::new(strip_trailing_slash(location), String::new()).with_line_number(line_number)
}

fn parse_entry_line(line: &str, line_number: usize) -> Option<PathEntry> {
    let trimmed = line.trim();

    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let parts = split_store_fields(line);

    let entry = match parts.as_slice() {
        [location, name] => {
            let parsed_location = strip_single_quotes(location);
            let parsed_name = parse_name_field(name);
            match (parsed_location, parsed_name) {
                (Some(location), Some(name)) => {
                    PathEntry::new(strip_trailing_slash(location), name)
                        .with_line_number(line_number)
                }
                (None, _) => {
                    eprintln!(
                        "warning: malformed entry at line {}: location must be wrapped in single quotes",
                        line_number
                    );
                    malformed_nameless_entry(location, line_number)
                }
                (_, None) => {
                    eprintln!(
                        "warning: malformed entry at line {}: name must be wrapped in '[' and ']'",
                        line_number
                    );
                    malformed_nameless_entry(location, line_number)
                }
            }
        }
        [location, name, options] => {
            let parsed_location = strip_single_quotes(location);
            let parsed_name = parse_name_field(name);
            let parsed_options = parse_entry_options(options);

            match (parsed_location, parsed_name, parsed_options) {
                (
                    Some(location),
                    Some(name),
                    EntryOptionsParseResult::Valid {
                        autoset,
                        prepend,
                        protect,
                        invalid_option,
                    },
                ) => {
                    let mut entry = PathEntry::new(strip_trailing_slash(location), name)
                        .with_options(autoset, prepend, protect)
                        .with_line_number(line_number);
                    entry.invalid_option = invalid_option;
                    if entry.invalid_option.is_some() {
                        entry = entry.with_original_options(options.as_str());
                    }
                    entry
                }
                (None, _, _) => {
                    eprintln!(
                        "warning: malformed entry at line {}: location must be wrapped in single quotes",
                        line_number
                    );
                    malformed_nameless_entry(location, line_number)
                }
                (_, None, _) => {
                    eprintln!(
                        "warning: malformed entry at line {}: name must be wrapped in '[' and ']'",
                        line_number
                    );
                    malformed_nameless_entry(location, line_number)
                }
                (_, _, EntryOptionsParseResult::Malformed) => {
                    eprintln!(
                        "warning: malformed entry at line {}: options must be wrapped in '(' and ')'",
                        line_number
                    );
                    malformed_nameless_entry(location, line_number)
                }
            }
        }
        [location] => {
            eprintln!(
                "warning: malformed entry at line {}: missing required name field",
                line_number
            );
            malformed_nameless_entry(location, line_number)
        }
        _ => {
            eprintln!(
                "warning: malformed entry at line {}: expected '<location>' [<name>] (<options>) (or omit options to default to auto/post)",
                line_number
            );
            malformed_nameless_entry(trimmed, line_number)
        }
    };

    Some(entry)
}

/// Serialize an entry back into a line.
fn format_entry_line(entry: &PathEntry) -> String {
    if entry.invalid_option.is_some() {
        if let Some(options_token) = entry.original_options.as_deref() {
            return format!(
                "{} [{}] {}",
                quote_store_location_field(&entry.location),
                entry.name,
                options_token
            );
        }
    }

    format!(
        "{} [{}] ({})",
        quote_store_location_field(&entry.location),
        entry.name,
        format_entry_options(entry)
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

/// Load entries or print an error and terminate.
fn load_entries_or_exit(store_file: &Path) -> Vec<PathEntry> {
    match load_entries(store_file) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("error: failed to load entries: {}", error);
            std::process::exit(1);
        }
    }
}

/// Load entries or print a warning and continue.
fn load_entries_or_warn(store_file: &Path, warning_context: &str) -> Option<Vec<PathEntry>> {
    match load_entries(store_file) {
        Ok(entries) => Some(entries),
        Err(error) => {
            eprintln!("warning: {}: {}", warning_context, error);
            None
        }
    }
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

fn report_invalid_option(level: &str, store_file: &Path, entry: &PathEntry) {
    let invalid_option = entry.invalid_option.as_deref().unwrap_or("<unknown>");
    eprintln!(
        "{}: unknown entry option '{}' at line {} in {}",
        level,
        invalid_option,
        entry.line_number,
        store_file.display()
    );
    eprintln!("{}: {}", level, format_entry_line(entry));
}

/// Check the existing entries, reporting any whose `location` does not
/// currently exist. Nameless/duplicate names are fatal errors; missing
/// locations are warnings only. Unknown option handling is controlled by the
/// caller.
fn validate_loaded_entries(store_file: &Path, entries: &[PathEntry], fail_on_invalid_option: bool) {
    let mut invalid_entries = entries
        .iter()
        .filter(|entry| entry.invalid_option.is_some())
        .peekable();
    if invalid_entries.peek().is_some() {
        for entry in invalid_entries {
            report_invalid_option(
                if fail_on_invalid_option {
                    "error"
                } else {
                    "warning"
                },
                store_file,
                entry,
            );
        }

        if fail_on_invalid_option {
            std::process::exit(1);
        }
    }

    if let Some(e) = entries.iter().find(|e| e.name.is_empty()) {
        eprintln!(
            "error: found nameless entry in {} at line {}: '{}'",
            store_file.display(),
            e.line_number,
            e.location
        );
        std::process::exit(1);
    }

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

    if let Some(e) = entries.iter().find(|e| !is_valid_name(&e.name)) {
        eprintln!(
            "error: invalid name '{}' at line {}: names must contain only alphanumeric characters",
            e.name, e.line_number
        );
        std::process::exit(1);
    }

    if let Some(e) = entries
        .iter()
        .find(|e| find_system_path_by_name(&e.name).is_some())
    {
        eprintln!(
            "error: name '{}' at line {} is reserved for a protected system path",
            e.name, e.line_number
        );
        std::process::exit(1);
    }

    let mut seen_names = std::collections::HashMap::new();
    for e in entries {
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

    let mut seen_locations = std::collections::HashMap::new();
    for entry in entries {
        seen_locations
            .entry(&entry.location)
            .or_insert_with(Vec::new)
            .push(entry.line_number);
    }

    let duplicate_locations: Vec<_> = seen_locations
        .iter()
        .filter(|(_, lines)| lines.len() > 1)
        .collect();
    if !duplicate_locations.is_empty() {
        for (location, lines) in duplicate_locations {
            let line_list = lines
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "error: duplicate path '{}' found at lines: {}",
                location, line_list
            );
        }
        std::process::exit(1);
    }

    let invalid: Vec<&PathEntry> = entries
        .iter()
        .filter(|e| !Path::new(&e.location).exists())
        .collect();
    if invalid.is_empty() {
        return;
    }

    eprintln!("warning: the following stored paths do not exist:");
    for e in invalid {
        eprintln!("  {}", e.location);
    }
}

/// Validate the configured store file entries.
///
/// Returns an I/O error only if loading entries fails.
fn validate_entries(store_file: &Path) -> io::Result<()> {
    let entries = load_entries(store_file)?;
    validate_loaded_entries(store_file, &entries, false);
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
                )
                .arg(
                    Arg::with_name("protect")
                        .long("protect")
                        .help("Store this named entry as protected from 'path remove'")
                        .takes_value(false),
                ),
        )
        .subcommand(
            SubCommand::with_name("remove")
                .about("Remove a directory from PATH")
                .arg(
                    Arg::with_name("force")
                        .short("f")
                        .long("force")
                        .help("Allow removing protected entries and built-in system paths")
                        .takes_value(false),
                )
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
            SubCommand::with_name("list")
                .about("List entries stored in the configured store file")
                .arg(
                    Arg::with_name("pretty")
                        .long("pretty")
                        .help("Print PATH entries as a formatted two-column table with names")
                        .takes_value(false),
                ),
        )
        .subcommand(
            SubCommand::with_name("load")
                .about("Add all auto entries from the configured store file to PATH"),
        )
        .subcommand(
            SubCommand::with_name("verify")
                .about("Validate configured store entries and report status"),
        )
        .subcommand(
            SubCommand::with_name("restore")
                .about("Restore standard protected system paths into PATH"),
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

/// Return whether a path-like value contains a backslash.
fn contains_backslash(path: &str) -> bool {
    path.contains('\\')
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

/// Validate a CLI path argument and return its canonicalized form.
fn validate_and_canonicalize_cli_path_argument(path: &str) -> String {
    if !is_path_argument_valid(path) {
        eprintln!("error: path '{}' must be absolute or start with '.'", path);
        std::process::exit(1);
    }

    if contains_path_separator(path) {
        eprintln!("error: path '{}' must not contain ':'", path);
        std::process::exit(1);
    }

    if contains_backslash(path) {
        eprintln!("error: path '{}' must not contain '\\\\'", path);
        std::process::exit(1);
    }

    canonicalize_relative_cli_argument(path)
}

/// Return whether a stored location is absolute and canonical-looking.
///
/// Canonical-looking means it is absolute, contains no `.`/`..` components,
/// has no duplicate separators, and has no trailing slash (except `/`).
fn is_store_location_canonical_like(location: &str) -> bool {
    if !location.starts_with('/') {
        return false;
    }

    // Disallow control characters, store syntax delimiters, and shell
    // metacharacters in locations to prevent parser ambiguity and shell
    // injection via crafted entries.
    if location.chars().any(|character| {
        character.is_ascii_control()
            || matches!(
                character,
                '[' | ']'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '`'
                    | ';'
                    | '$'
                    | '!'
                    | '&'
                    | '|'
                    | '<'
                    | '>'
                    | '"'
                    | '\''
                    | '\\'
                    | '*'
                    | '?'
                    | '#'
                    | '~'
                    | '^'
            )
    }) {
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
    format!(
        "{} [{}] ({})",
        entry.location,
        entry.name,
        format_entry_options(entry)
    )
}

/// Handle the `add` subcommand.
///
/// This resolves named entries, validates path shape, optionally persists a
/// named mapping, and prints the resulting PATH export command.
fn handle_add(add_matches: &ArgMatches, store_file: &Path) {
    let mut location = add_matches.value_of("location").unwrap().to_string();
    let mut resolved_by_name = false;
    let prepend = add_matches.is_present("pre");

    if let Some(entries) = load_entries_or_warn(
        store_file,
        "failed to load store file, treating argument as path",
    ) {
        if let Some(resolved_location) = resolve_location_by_name(&location, &entries) {
            location = resolved_location;
            resolved_by_name = true;
        }
    }

    if !resolved_by_name {
        location = validate_and_canonicalize_cli_path_argument(&location);
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
    let protect = add_matches.is_present("protect");

    if let Some(name) = name_opt {
        if !is_valid_name(&name) {
            eprintln!(
                "error: invalid name '{}': names must contain only alphanumeric characters",
                name
            );
            std::process::exit(1);
        }

        if find_system_path_by_name(&name).is_some() {
            eprintln!(
                "error: name '{}' is reserved for a protected system path",
                name
            );
            std::process::exit(1);
        }

        if let Some(mut entries) = load_entries_or_warn(
            store_file,
            "failed to load store file, not updating named entries",
        ) {
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

            entries.push(
                PathEntry::new(location.clone(), name).with_options(autoset, prepend, protect),
            );

            if let Err(error) = save_entries(store_file, &entries) {
                eprintln!("warning: failed to update store file: {}", error);
            }
        }
    }

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
    let force_remove = remove_matches.is_present("force");

    let mut location_to_remove = argument.clone();
    let mut resolved_by_name = false;
    let loaded_entries = load_entries(store_file).ok();

    if let Some(system_entry) = find_system_path_by_name(&argument) {
        if !force_remove {
            eprintln!(
                "error: system path '{}' ({}) is protected and cannot be removed with 'path remove'",
                system_entry.location, system_entry.name
            );
            std::process::exit(1);
        }

        location_to_remove = system_entry.location.clone();
        resolved_by_name = true;
    }

    if let Some(entries) = loaded_entries.as_ref().filter(|_| !resolved_by_name) {
        if let Some(entry) = entries.iter().find(|entry| entry.name == argument) {
            if entry.protect && !force_remove {
                eprintln!(
                    "error: entry '{}' is protected and cannot be removed with 'path remove'",
                    argument
                );
                std::process::exit(1);
            }

            location_to_remove = entry.location.clone();
            resolved_by_name = true;
        }
    }

    let mut raw_path_arg = None;
    if !resolved_by_name {
        location_to_remove = validate_and_canonicalize_cli_path_argument(&argument);
        raw_path_arg = Some(argument.as_str());

        if let Some(system_entry) =
            find_system_path_by_location(&location_to_remove).filter(|_| !force_remove)
        {
            eprintln!(
                "error: system path '{}' ({}) is protected and cannot be removed with 'path remove'",
                system_entry.location, system_entry.name
            );
            std::process::exit(1);
        }

        if let Some(entries) = loaded_entries.as_ref() {
            if let Some(entry) = entries.iter().find(|entry| {
                entry.protect && entry.location == location_to_remove && !force_remove
            }) {
                eprintln!(
                    "error: entry '{}' is protected and cannot be removed with 'path remove'",
                    entry.name
                );
                std::process::exit(1);
            }
        }
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

    let mut entries = load_entries_or_exit(store_file);

    if let Some(position) = entries.iter().position(|entry| entry.name == argument) {
        entries.remove(position);
    } else {
        let location_to_delete = validate_and_canonicalize_cli_path_argument(&argument);
        entries.retain(|entry| entry.location != location_to_delete);
    }

    if let Err(error) = save_entries(store_file, &entries) {
        eprintln!("warning: failed to update store file: {}", error);
    }
}

/// Handle the `list` subcommand by printing all stored entries.
///
/// When `--pretty` is given, enumerates the current PATH segments in a
/// table with index, path, type, and name columns, using names resolved
/// from the store file and the built-in system path list.
fn handle_list(list_matches: &ArgMatches, store_file: &Path) {
    if list_matches.is_present("pretty") {
        let current = env::var("PATH").unwrap_or_default();

        let entries = load_entries_or_warn(store_file, "failed to load store file for --pretty")
            .unwrap_or_default();

        let segments: Vec<&str> = if current.is_empty() {
            Vec::new()
        } else {
            current.split(':').collect()
        };

        let names: Vec<String> = segments
            .iter()
            .map(|seg| resolve_segment_name(seg, &entries))
            .collect();

        let types: Vec<String> = segments
            .iter()
            .map(|seg| resolve_segment_type(seg, &entries))
            .collect();

        let index_col_width = segments.len().to_string().len().max("#".len());

        let path_col_width = segments
            .iter()
            .map(|s| s.len())
            .max()
            .unwrap_or(0)
            .max("PATH".len());

        let type_col_width = types
            .iter()
            .map(|t| t.len())
            .max()
            .unwrap_or(0)
            .max("TYPE".len());

        let name_col_width = names
            .iter()
            .map(|n| n.len())
            .max()
            .unwrap_or(0)
            .max("NAME".len());

        println!(
            "{:<index_width$}  {:<path_width$}  {:<type_width$}  NAME",
            "#",
            "PATH",
            "TYPE",
            index_width = index_col_width,
            path_width = path_col_width,
            type_width = type_col_width
        );
        println!(
            "{:-<index_width$}  {:-<path_width$}  {:-<type_width$}  {:-<name_width$}",
            "",
            "",
            "",
            "",
            index_width = index_col_width,
            path_width = path_col_width,
            type_width = type_col_width,
            name_width = name_col_width
        );

        for (index, ((segment, entry_type), name)) in segments
            .iter()
            .zip(types.iter())
            .zip(names.iter())
            .enumerate()
        {
            println!(
                "{:<index_width$}  {:<path_width$}  {:<type_width$}  {}",
                index + 1,
                segment,
                entry_type,
                name,
                index_width = index_col_width,
                path_width = path_col_width,
                type_width = type_col_width
            );
        }
        return;
    }

    let store_exists = store_file.exists();

    let entries = load_entries_or_exit(store_file);
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

/// Handle the `load` subcommand.
///
/// This applies all entries marked `auto` in the configured store file to PATH,
/// prepending entries marked `pre` and appending all others, while skipping
/// entries already present as exact path segments.
fn handle_load(store_file: &Path) {
    let entries = load_entries_or_exit(store_file);

    let mut current = env::var("PATH").unwrap_or_default();
    for entry in entries.into_iter().filter(|entry| entry.autoset) {
        if !path_contains_segment(&current, &entry.location) {
            current = compose_path(&current, &entry.location, entry.prepend);
        }
    }

    println!("{}", format_export_path(&current));
}

/// Handle the `verify` subcommand.
///
/// Validation errors are emitted by `validate_entries`; when no failures are
/// found this prints a short success message.
fn handle_verify(store_file: &Path) {
    if !store_file.exists() {
        eprintln!("error: store file does not exist: {}", store_file.display());
        std::process::exit(1);
    }

    let entries = load_entries_or_exit(store_file);

    if entries.is_empty() {
        eprintln!("error: store file has no entries: {}", store_file.display());
        std::process::exit(1);
    }

    validate_loaded_entries(store_file, &entries, true);
    println!("Path file is valid.");
}

/// Restore the built-in protected system path entries to PATH without
/// persisting them to the store file.
fn handle_restore() {
    let mut current = env::var("PATH").unwrap_or_default();

    for system_entry in standard_system_paths() {
        if !path_contains_segment(&current, &system_entry.location) {
            current = compose_path(&current, &system_entry.location, false);
        }
    }

    println!("{}", format_export_path(&current));
}

/// Resolve the display name for a PATH segment.
///
/// Checks stored entries first, then the built-in system path table, then
/// the known extra paths table.  Returns an empty string when no name is
/// known.
fn resolve_segment_name(segment: &str, entries: &[PathEntry]) -> String {
    let normalized = strip_trailing_slash(segment);

    if let Some(entry) = entries
        .iter()
        .find(|e| strip_trailing_slash(&e.location) == normalized)
    {
        return entry.name.clone();
    }

    if let Some(system) = find_system_path_by_location(&normalized) {
        return system.name.to_string();
    }

    if let Some(extra) = find_extra_path_by_location(&normalized) {
        return extra.name;
    }

    String::new()
}

/// Resolve the display type for a PATH segment.
///
/// Type labels are `system` and `known`. Non-built-in segments have a blank
/// type unless they are protected store entries, in which case this returns
/// `[protected]`.
fn resolve_segment_type(segment: &str, entries: &[PathEntry]) -> String {
    let normalized = strip_trailing_slash(segment);

    if let Some(system) = find_system_path_by_location(&normalized) {
        if system.protect {
            return "system [protected]".to_string();
        }
        return "system".to_string();
    }

    if find_extra_path_by_location(&normalized).is_some() {
        return "known".to_string();
    }

    if entries
        .iter()
        .any(|entry| strip_trailing_slash(&entry.location) == normalized && entry.protect)
    {
        return "[protected]".to_string();
    }

    String::new()
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
/// Validates stored entries for subcommands that need them, dispatches subcommands,
/// and falls back to printing the current PATH when no subcommand is provided.
fn main() {
    let matches = build_cli().get_matches();
    let store_file = resolve_store_file_path(&matches);

    if matches.subcommand_matches("verify").is_some() {
        handle_verify(&store_file);
        return;
    }

    // Subcommands that need the store file: add, remove, delete, list, load.
    // Store validation is skipped for restore and default (print_current_path),
    // which don't use the store, so a malformed store file won't block recovery.
    let needs_store = matches.subcommand_matches("add").is_some()
        || matches.subcommand_matches("remove").is_some()
        || matches.subcommand_matches("delete").is_some()
        || matches.subcommand_matches("list").is_some()
        || matches.subcommand_matches("load").is_some();

    if needs_store {
        if let Err(error) = validate_entries(&store_file) {
            eprintln!("warning: could not validate entries: {}", error);
        }
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

    if let Some(list_matches) = matches.subcommand_matches("list") {
        handle_list(list_matches, &store_file);
        return;
    }

    if matches.subcommand_matches("load").is_some() {
        handle_load(&store_file);
        return;
    }

    if matches.subcommand_matches("restore").is_some() {
        handle_restore();
        return;
    }

    print_current_path();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to construct a test entry with sensible defaults.
    fn test_entry(location: &str, name: &str) -> PathEntry {
        PathEntry::new(location, name)
    }

    #[test]
    /// Ensure `PathEntry::new` applies the standard default option state.
    fn path_entry_new_uses_default_values() {
        let entry = PathEntry::new("/tmp/tools", "tools");

        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "tools");
        assert!(entry.autoset);
        assert!(!entry.prepend);
        assert!(!entry.protect);
        assert!(entry.invalid_option.is_none());
        assert!(entry.original_options.is_none());
        assert_eq!(entry.line_number, 0);
    }

    #[test]
    /// Ensure `with_options` updates only the explicit option flags.
    fn path_entry_with_options_updates_option_flags() {
        let entry = PathEntry::new("/tmp/tools", "tools").with_options(false, true, true);

        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "tools");
        assert!(!entry.autoset);
        assert!(entry.prepend);
        assert!(entry.protect);
        assert!(entry.invalid_option.is_none());
        assert!(entry.original_options.is_none());
        assert_eq!(entry.line_number, 0);
    }

    #[test]
    /// Ensure `with_line_number` records source line metadata.
    fn path_entry_with_line_number_updates_line_metadata() {
        let entry = PathEntry::new("/tmp/tools", "tools").with_line_number(4);

        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "tools");
        assert!(entry.autoset);
        assert!(!entry.prepend);
        assert!(!entry.protect);
        assert!(entry.original_options.is_none());
        assert_eq!(entry.line_number, 4);
    }

    #[test]
    /// Ensure `with_original_options` stores the raw options token for round-trip output.
    fn path_entry_with_original_options_preserves_raw_token() {
        let entry = PathEntry::new("/tmp/tools", "tools")
            .with_options(false, true, true)
            .with_original_options("(noauto,pre,protect,postfix)");

        assert_eq!(
            entry.original_options.as_deref(),
            Some("(noauto,pre,protect,postfix)")
        );
    }

    #[test]
    /// Ensure single-quoted values are unwrapped correctly.
    fn strip_single_quotes_strips_delimiters() {
        assert_eq!(strip_single_quotes("'foo'"), Some("foo"));
        assert_eq!(strip_single_quotes("''"), Some(""));
    }

    #[test]
    /// Ensure generic wrapped-field stripping handles different delimiters.
    fn strip_wrapped_strips_matching_delimiters() {
        assert_eq!(strip_wrapped("'foo'", '\'', '\''), Some("foo"));
        assert_eq!(strip_wrapped("[name]", '[', ']'), Some("name"));
        assert_eq!(strip_wrapped("(auto)", '(', ')'), Some("auto"));
    }

    #[test]
    /// Ensure generic wrapped-field stripping rejects malformed delimiter shapes.
    fn strip_wrapped_rejects_invalid_shape() {
        assert_eq!(strip_wrapped("foo", '\'', '\''), None);
        assert_eq!(strip_wrapped("'foo", '\'', '\''), None);
        assert_eq!(strip_wrapped("foo'", '\'', '\''), None);
        assert_eq!(strip_wrapped("[name)", '[', ']'), None);
    }

    #[test]
    /// Ensure malformed or unquoted values are rejected.
    fn strip_single_quotes_rejects_invalid_shape() {
        assert_eq!(strip_single_quotes("foo"), None);
        assert_eq!(strip_single_quotes("'foo"), None);
        assert_eq!(strip_single_quotes("foo'"), None);
    }

    #[test]
    /// Verify three-field lines are parsed into all struct fields.
    fn parse_entry_line_parses_three_fields() {
        let entry = parse_entry_line("'/tmp/tools/' [tools] (noauto)", 7).unwrap();
        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "tools");
        assert!(!entry.autoset);
        assert!(!entry.prepend);
        assert!(!entry.protect);
        assert_eq!(entry.line_number, 7);
    }

    #[test]
    /// Ensure unwrapped legacy field formats are rejected.
    fn parse_entry_line_legacy_unwrapped_fields_are_rejected() {
        let entry = parse_entry_line("/tmp/tools\ttools\tnoauto", 8).unwrap();
        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "");
        assert!(entry.autoset);
        assert!(!entry.prepend);
        assert!(!entry.protect);
    }

    #[test]
    /// Verify lines without a name field become nameless entries for validation.
    fn parse_entry_line_without_name_field_creates_nameless_entry() {
        let entry = parse_entry_line("/tmp/tools", 3).unwrap();
        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "");
        assert!(entry.autoset);
        assert!(!entry.prepend);
        assert!(!entry.protect);
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
        assert!(!is_store_location_canonical_like("/tmp/[evil]"));
        assert!(!is_store_location_canonical_like("/tmp/evil]"));
        assert!(!is_store_location_canonical_like("/tmp/[evil"));
        assert!(!is_store_location_canonical_like("/tmp/(evil)"));
        assert!(!is_store_location_canonical_like("/tmp/evil("));
        assert!(!is_store_location_canonical_like("/tmp/evil)"));
        assert!(!is_store_location_canonical_like("/tmp/{evil}"));
        assert!(!is_store_location_canonical_like("/tmp/`evil`"));
        assert!(!is_store_location_canonical_like("/tmp/evil`"));
        assert!(!is_store_location_canonical_like("/tmp/ev;il"));
        assert!(!is_store_location_canonical_like("/tmp/$evil"));
        assert!(!is_store_location_canonical_like("/tmp/ev|il"));
        assert!(!is_store_location_canonical_like("/tmp/ev*il"));
        assert!(!is_store_location_canonical_like("/tmp/ev?il"));
        assert!(!is_store_location_canonical_like("/tmp/ev!il"));
        assert!(!is_store_location_canonical_like("/tmp/ev&il"));
        assert!(!is_store_location_canonical_like("/tmp/ev<il"));
        assert!(!is_store_location_canonical_like("/tmp/ev>il"));
        assert!(!is_store_location_canonical_like("/tmp/ev\"il"));
        assert!(!is_store_location_canonical_like("/tmp/ev'il"));
        assert!(!is_store_location_canonical_like("/tmp/ev\\il"));
        assert!(!is_store_location_canonical_like("/tmp/ev#il"));
        assert!(!is_store_location_canonical_like("/tmp/ev~il"));
        assert!(!is_store_location_canonical_like("/tmp/ev^il"));
        assert!(!is_store_location_canonical_like("/tmp/ev\til")); // tab
        assert!(!is_store_location_canonical_like("/tmp/ev\nil")); // newline
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
    /// Verify that all built-in path names are unique.
    ///
    /// This test enforces the invariant using the same `PathEntry` values that
    /// power the runtime built-in path tables.
    fn builtin_path_names_are_unique() {
        assert!(builtins_have_unique_names());
    }

    #[test]
    /// Verify stored name lookup returns the expected location when present.
    fn resolve_location_by_name_returns_matching_location() {
        let mut entry = test_entry("/usr/local/bin", "tools");
        entry.line_number = 1;
        let entries = vec![entry];
        assert_eq!(
            resolve_location_by_name("tools", &entries),
            Some("/usr/local/bin".to_string())
        );
        assert_eq!(resolve_location_by_name("missing", &entries), None);
    }

    #[test]
    /// Ensure known_extra_paths contains the expected static and HOME-relative entries.
    fn known_extra_paths_contains_expected_entries() {
        let extras = known_extra_paths();
        let locations: Vec<&str> = extras.iter().map(|e| e.location.as_str()).collect();
        assert!(locations.contains(&"/opt/homebrew/bin"));
        assert!(locations.contains(&"/opt/homebrew/sbin"));
        // All extra entries must be unprotected.
        assert!(extras.iter().all(|e| !e.protect));
        // HOME-relative entries are present when HOME is set.
        if let Ok(home) = env::var("HOME") {
            let cargo = format!("{}/.cargo/bin", home);
            let pipx = format!("{}/.local/bin", home);
            assert!(locations.contains(&cargo.as_str()));
            assert!(locations.contains(&pipx.as_str()));
        }
    }

    #[test]
    /// Ensure built-in system and extra paths have the expected default flags.
    fn built_in_paths_use_expected_option_flags() {
        assert!(standard_system_paths().iter().all(|entry| {
            entry.autoset && !entry.prepend && entry.protect && entry.invalid_option.is_none()
        }));

        let extras = known_extra_paths();
        assert!(extras.iter().all(|entry| {
            entry.autoset && !entry.prepend && !entry.protect && entry.invalid_option.is_none()
        }));
    }

    #[test]
    /// Ensure find_extra_path_by_location returns the matching entry and None for unknowns.
    fn find_extra_path_by_location_returns_correct_entry() {
        let entry = find_extra_path_by_location("/opt/homebrew/bin").unwrap();
        assert_eq!(entry.name, "homebrewbin");
        assert!(!entry.protect);

        assert!(find_extra_path_by_location("/no/such/path").is_none());
    }

    #[test]
    /// Ensure known Homebrew segments get friendly names in pretty output.
    fn resolve_segment_name_resolves_homebrew_extras() {
        let entries: Vec<PathEntry> = Vec::new();
        assert_eq!(
            resolve_segment_name("/opt/homebrew/bin", &entries),
            "homebrewbin"
        );
        assert_eq!(
            resolve_segment_name("/opt/homebrew/sbin", &entries),
            "homebrewsbin"
        );
    }

    #[test]
    /// Ensure known user-local segments for Cargo and pipx are recognized.
    fn resolve_segment_name_resolves_cargo_and_pipx_extras() {
        let entries: Vec<PathEntry> = Vec::new();
        if let Ok(home) = env::var("HOME") {
            let cargo_bin = format!("{}/.cargo/bin", home);
            let pipx_bin = format!("{}/.local/bin", home);
            assert_eq!(resolve_segment_name(&cargo_bin, &entries), "cargo");
            assert_eq!(resolve_segment_name(&pipx_bin, &entries), "pipx");
        }
    }

    #[test]
    /// Ensure system and known segments resolve to their expected type labels.
    fn resolve_segment_type_resolves_system_and_known() {
        let entries: Vec<PathEntry> = Vec::new();
        assert_eq!(
            resolve_segment_type("/usr/bin", &entries),
            "system [protected]"
        );
        assert_eq!(resolve_segment_type("/opt/homebrew/bin", &entries), "known");
    }

    #[test]
    /// Ensure protected store entries are marked as protected in pretty type output.
    fn resolve_segment_type_marks_protected_store_entry() {
        let entries = vec![PathEntry {
            location: "/opt/locked".to_string(),
            name: "locked".to_string(),
            autoset: true,
            prepend: false,
            protect: true,
            invalid_option: None,
            original_options: None,
            line_number: 1,
        }];

        assert_eq!(resolve_segment_type("/opt/locked", &entries), "[protected]");
        assert_eq!(resolve_segment_type("/opt/open", &entries), "");
    }

    #[test]
    /// Ensure list formatting mirrors store layout with unquoted locations.
    fn format_list_entry_includes_name_when_different() {
        let entry = test_entry("/usr/local/bin", "tools");
        assert_eq!(format_list_entry(&entry), "/usr/local/bin [tools] (auto)");

        let mut same = test_entry("/usr/bin", "/usr/bin");
        same.autoset = false;
        assert_eq!(format_list_entry(&same), "/usr/bin [/usr/bin] (noauto)");

        let mut pre = test_entry("/opt/pre", "pre");
        pre.prepend = true;
        assert_eq!(format_list_entry(&pre), "/opt/pre [pre] (auto,pre)");

        let mut protect = test_entry("/opt/protect", "protect");
        protect.protect = true;
        assert_eq!(
            format_list_entry(&protect),
            "/opt/protect [protect] (auto,protect)"
        );
    }

    #[test]
    /// Ensure missing third fields are interpreted as `auto` for manual edits.
    fn parse_entry_line_missing_third_field_defaults_to_auto() {
        let entry = parse_entry_line("'/tmp/tools' [tools]", 2).unwrap();
        assert!(entry.autoset);
        assert!(!entry.prepend);
    }

    #[test]
    /// Ensure `(pre)` enables prepend while defaulting autoset to `auto`.
    fn parse_entry_line_pre_option_enables_prepend() {
        let entry = parse_entry_line("'/tmp/tools' [tools] (pre)", 2).unwrap();
        assert!(entry.autoset);
        assert!(entry.prepend);
        assert!(!entry.protect);
    }

    #[test]
    /// Ensure comma-delimited options parse both autoset and prepend settings.
    fn parse_entry_line_combined_options_parse() {
        let entry = parse_entry_line("'/tmp/tools' [tools] (noauto,pre)", 2).unwrap();
        assert!(!entry.autoset);
        assert!(entry.prepend);
        assert!(!entry.protect);
    }

    #[test]
    /// Ensure `protect` is parsed as a supported option.
    fn parse_entry_line_protect_option_enables_protection() {
        let entry = parse_entry_line("'/tmp/tools' [tools] (auto,protect)", 2).unwrap();
        assert!(entry.autoset);
        assert!(!entry.prepend);
        assert!(entry.protect);
    }

    #[test]
    /// Ensure options containing braces are rejected as malformed.
    fn parse_entry_line_options_with_braces_are_rejected() {
        let entry = parse_entry_line("'/tmp/tools' [tools] (auto,{pre})", 2).unwrap();
        assert_eq!(entry.name, "");
    }

    #[test]
    /// Ensure options containing nested parentheses are rejected as malformed.
    fn parse_entry_line_options_with_nested_parentheses_are_rejected() {
        let entry = parse_entry_line("'/tmp/tools' [tools] (auto,(pre))", 2).unwrap();
        assert_eq!(entry.name, "");
    }

    #[test]
    /// Ensure asymmetric bracket delimiters in names are rejected.
    fn parse_entry_line_name_with_asymmetric_brackets_is_rejected() {
        let missing_close = parse_entry_line("'/tmp/tools' [tools (auto)", 2).unwrap();
        assert_eq!(missing_close.name, "");

        let missing_open = parse_entry_line("'/tmp/tools' tools] (auto)", 3).unwrap();
        assert_eq!(missing_open.name, "");
    }

    #[test]
    /// Ensure asymmetric parenthesis delimiters in options are rejected.
    fn parse_entry_line_options_with_asymmetric_parentheses_are_rejected() {
        let missing_close = parse_entry_line("'/tmp/tools' [tools] (auto", 2).unwrap();
        assert_eq!(missing_close.name, "");

        let missing_open = parse_entry_line("'/tmp/tools' [tools] auto)", 3).unwrap();
        assert_eq!(missing_open.name, "");
    }

    #[test]
    /// Ensure unknown alphabetic options are preserved for diagnostics while
    /// recognized options still take effect.
    fn parse_entry_line_postfix_option_preserves_valid_options() {
        let line = "'/tmp/tools' [tools] (noauto,pre,protect,postfix)";
        let entry = parse_entry_line(line, 2).unwrap();
        assert_eq!(entry.name, "tools");
        assert!(!entry.autoset);
        assert!(entry.prepend);
        assert!(entry.protect);
        assert_eq!(entry.invalid_option.as_deref(), Some("postfix"));
        assert_eq!(
            entry.original_options.as_deref(),
            Some("(noauto,pre,protect,postfix)")
        );
        assert_eq!(format_entry_line(&entry), line);
    }

    #[test]
    /// Ensure quoted locations preserve literal spaces in stored paths.
    fn parse_entry_line_quoted_location_preserves_spaces() {
        let entry = parse_entry_line("'/tmp/my tools' [tools] (auto)", 4).unwrap();
        assert_eq!(entry.location, "/tmp/my tools");
        assert_eq!(entry.name, "tools");
        assert!(entry.autoset);
        assert!(!entry.prepend);
        assert!(!entry.protect);
    }

    #[test]
    /// Ensure location parsing does not decode backslash escape sequences.
    fn parse_entry_line_preserves_backslashes_for_validation() {
        let entry = parse_entry_line("'/tmp/my\\\\tools' [tools] (auto)", 4).unwrap();
        assert_eq!(entry.location, "/tmp/my\\\\tools");
    }

    #[test]
    /// Ensure comment lines in the store file are ignored by the parser.
    fn parse_entry_line_comment_line_is_ignored() {
        assert!(parse_entry_line("# layout: '<location>' [<name>] (<options>)", 1).is_none());
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
        let auto = test_entry("/tmp/a", "a");
        assert_eq!(format_entry_line(&auto), "'/tmp/a' [a] (auto)");

        let mut noauto = test_entry("/tmp/b", "b");
        noauto.autoset = false;
        assert_eq!(format_entry_line(&noauto), "'/tmp/b' [b] (noauto)");

        let mut pre = test_entry("/tmp/c", "c");
        pre.prepend = true;
        assert_eq!(format_entry_line(&pre), "'/tmp/c' [c] (auto,pre)");

        let mut protect = test_entry("/tmp/d", "d");
        protect.protect = true;
        assert_eq!(format_entry_line(&protect), "'/tmp/d' [d] (auto,protect)");
    }

    #[test]
    /// Ensure serializer wraps locations in quotes without escape processing.
    fn format_entry_line_quotes_location_without_escape_processing() {
        let spaced = test_entry("/tmp/my tools", "tools");
        assert_eq!(format_entry_line(&spaced), "'/tmp/my tools' [tools] (auto)");

        let backslash = test_entry("/tmp/my\\tools", "tools");
        assert_eq!(
            format_entry_line(&backslash),
            "'/tmp/my\\tools' [tools] (auto)"
        );
    }
}
