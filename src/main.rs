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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutosetMode {
    Auto,
    NoAuto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlacementMode {
    Prefix,
    Postfix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProtectionMode {
    Protected,
    Unprotected,
}

/// Simple representation of a stored path entry in plain text form.
#[derive(Debug, Clone)]
struct PathEntry {
    location: String,
    name: String,
    auto_mode: AutosetMode,
    placement_mode: PlacementMode,
    protection_mode: ProtectionMode,
    line_number: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EntryDiagnosticKind {
    UnknownOption { option: String },
    ConflictingOptions { left: String, right: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryDiagnostic {
    line_number: usize,
    line: String,
    kind: EntryDiagnosticKind,
}

impl PathEntry {
    // Create an entry with the default option and metadata state.
    fn new(location: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            name: name.into(),
            auto_mode: AutosetMode::Auto,
            placement_mode: PlacementMode::Postfix,
            protection_mode: ProtectionMode::Unprotected,
            line_number: 0,
        }
    }

    // Apply parsed or CLI-provided option flags without touching metadata.
    fn with_options(
        mut self,
        auto_mode: AutosetMode,
        placement_mode: PlacementMode,
        protection_mode: ProtectionMode,
    ) -> Self {
        self.auto_mode = auto_mode;
        self.placement_mode = placement_mode;
        self.protection_mode = protection_mode;
        self
    }

    fn autoset_enabled(&self) -> bool {
        self.auto_mode == AutosetMode::Auto
    }

    fn prepends_path(&self) -> bool {
        self.placement_mode == PlacementMode::Prefix
    }

    fn is_protected(&self) -> bool {
        self.protection_mode == ProtectionMode::Protected
    }

    // Preserve source line-number metadata for diagnostics.
    fn with_line_number(mut self, line_number: usize) -> Self {
        self.line_number = line_number;
        self
    }
}

const DEFAULT_STORE_FILE_NAME: &str = ".path";
const STORE_FILE_LAYOUT_COMMENT: &str = "# layout: '<location>' [<name>] (<options>)";

/// Build a built-in path entry as a `PathEntry`.
fn builtin_path_entry(
    location: &str,
    name: &str,
    placement_mode: PlacementMode,
    protection_mode: ProtectionMode,
) -> PathEntry {
    PathEntry::new(location, name).with_options(AutosetMode::Auto, placement_mode, protection_mode)
}

/// Standard system paths managed by `path restore`.
fn standard_system_paths() -> &'static [PathEntry] {
    static SYSTEM_PATHS: OnceLock<Vec<PathEntry>> = OnceLock::new();
    SYSTEM_PATHS
        .get_or_init(|| {
            vec![
                builtin_path_entry(
                    "/bin",
                    "sysbin",
                    PlacementMode::Postfix,
                    ProtectionMode::Protected,
                ),
                builtin_path_entry(
                    "/sbin",
                    "syssbin",
                    PlacementMode::Postfix,
                    ProtectionMode::Protected,
                ),
                builtin_path_entry(
                    "/usr/bin",
                    "usrbin",
                    PlacementMode::Postfix,
                    ProtectionMode::Protected,
                ),
                builtin_path_entry(
                    "/usr/sbin",
                    "usrsbin",
                    PlacementMode::Postfix,
                    ProtectionMode::Protected,
                ),
                builtin_path_entry(
                    "/usr/local/bin",
                    "usrlocalbin",
                    PlacementMode::Postfix,
                    ProtectionMode::Protected,
                ),
                builtin_path_entry(
                    "/usr/local/sbin",
                    "usrlocalsbin",
                    PlacementMode::Postfix,
                    ProtectionMode::Protected,
                ),
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

/// Known non-system tool paths recognised for default pretty output.
///
/// These entries are unprotected and are not managed by `path restore`. The
/// `$HOME`-relative entries are expanded from the current environment at first call,
/// then cached for efficient reuse across path segment processing.
fn known_extra_paths() -> &'static [PathEntry] {
    static EXTRA_PATHS: OnceLock<Vec<PathEntry>> = OnceLock::new();
    EXTRA_PATHS
        .get_or_init(|| {
            let mut entries = vec![
                builtin_path_entry(
                    "/opt/homebrew/bin",
                    "homebrewbin",
                    PlacementMode::Postfix,
                    ProtectionMode::Unprotected,
                ),
                builtin_path_entry(
                    "/opt/homebrew/sbin",
                    "homebrewsbin",
                    PlacementMode::Postfix,
                    ProtectionMode::Unprotected,
                ),
            ];

            if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
                let home_str = strip_trailing_slash(&home.to_string_lossy());
                entries.push(builtin_path_entry(
                    &format!("{}/.cargo/bin", home_str),
                    "cargo",
                    PlacementMode::Postfix,
                    ProtectionMode::Unprotected,
                ));
                entries.push(builtin_path_entry(
                    &format!("{}/.local/bin", home_str),
                    "pipx",
                    PlacementMode::Postfix,
                    ProtectionMode::Unprotected,
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

/// Represent parsed options with their associated diagnostic information.
struct ParsedOptions {
    auto_mode: AutosetMode,
    placement_mode: PlacementMode,
    protection_mode: ProtectionMode,
    diagnostic: Option<EntryDiagnosticKind>,
}

/// Decode entry options from the third field.
///
/// Result for decoding a store entry options field.
enum EntryOptionsParseResult {
    Parsed(ParsedOptions),
    Malformed,
}

const MUTUALLY_EXCLUSIVE_OPTION_PAIRS: [(&str, &str); 2] = [("auto", "noauto"), ("pre", "post")];

/// Build a Parsed result seeded with default option modes and an optional diagnostic.
fn default_parsed_options_result(
    diagnostic: Option<EntryDiagnosticKind>,
) -> EntryOptionsParseResult {
    EntryOptionsParseResult::Parsed(ParsedOptions {
        auto_mode: AutosetMode::Auto,
        placement_mode: PlacementMode::Postfix,
        protection_mode: ProtectionMode::Unprotected,
        diagnostic,
    })
}

/// Construct an entry diagnostic from a parsed diagnostic kind and source metadata.
fn entry_diagnostic(line_number: usize, line: &str, kind: EntryDiagnosticKind) -> EntryDiagnostic {
    EntryDiagnostic {
        line_number,
        line: line.to_string(),
        kind,
    }
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
        return default_parsed_options_result(None);
    }

    let options: Vec<&str> = normalized
        .split(',')
        .map(str::trim)
        .filter(|opt| !opt.is_empty())
        .collect();

    for (left, right) in MUTUALLY_EXCLUSIVE_OPTION_PAIRS {
        if options.contains(&left) && options.contains(&right) {
            return default_parsed_options_result(Some(EntryDiagnosticKind::ConflictingOptions {
                left: left.to_string(),
                right: right.to_string(),
            }));
        }
    }

    let mut auto_mode = AutosetMode::Auto;
    let mut placement_mode = PlacementMode::Postfix;
    let mut protection_mode = ProtectionMode::Unprotected;
    let mut unknown_option = None;

    for option in options {
        match option {
            "auto" => auto_mode = AutosetMode::Auto,
            "noauto" => auto_mode = AutosetMode::NoAuto,
            "pre" => placement_mode = PlacementMode::Prefix,
            "post" => placement_mode = PlacementMode::Postfix,
            "protect" => protection_mode = ProtectionMode::Protected,
            other => {
                if other
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
                {
                    if unknown_option.is_none() {
                        unknown_option = Some(other.to_string());
                    }
                    continue;
                }
                return EntryOptionsParseResult::Malformed;
            }
        }
    }

    EntryOptionsParseResult::Parsed(ParsedOptions {
        auto_mode,
        placement_mode,
        protection_mode,
        diagnostic: unknown_option.map(|option| EntryDiagnosticKind::UnknownOption { option }),
    })
}

/// Render the option marker list for a stored entry.
fn format_entry_options(entry: &PathEntry) -> String {
    let autoset = match entry.auto_mode {
        AutosetMode::Auto => "auto",
        AutosetMode::NoAuto => "noauto",
    };
    let mut options = vec![autoset.to_string()];

    if entry.prepends_path() {
        options.push("pre".to_string());
    }

    if entry.is_protected() {
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

fn parse_entry_line_with_diagnostic(
    line: &str,
    line_number: usize,
) -> Option<(PathEntry, Option<EntryDiagnostic>, Option<String>)> {
    let trimmed = line.trim();

    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let parts = split_store_fields(line);

    let parsed = match parts.as_slice() {
        [location, name] => {
            let parsed_location = strip_single_quotes(location);
            let parsed_name = parse_name_field(name);
            match (parsed_location, parsed_name) {
                (Some(location), Some(name)) => (
                    PathEntry::new(strip_trailing_slash(location), name)
                        .with_line_number(line_number),
                    None,
                    None,
                ),
                (None, _) => (
                    malformed_nameless_entry(location, line_number),
                    None,
                    Some(format!(
                        "warning: malformed entry at line {}: location must be wrapped in single quotes",
                        line_number
                    )),
                ),
                (_, None) => (
                    malformed_nameless_entry(location, line_number),
                    None,
                    Some(format!(
                        "warning: malformed entry at line {}: name must be wrapped in '[' and ']'",
                        line_number
                    )),
                ),
            }
        }
        [location, name, options] => {
            let parsed_location = strip_single_quotes(location);
            let parsed_name = parse_name_field(name);
            let parsed_options = parse_entry_options(options);

            match (parsed_location, parsed_name, parsed_options) {
                (Some(location), Some(name), EntryOptionsParseResult::Parsed(options)) => {
                    let entry = PathEntry::new(strip_trailing_slash(location), name)
                        .with_options(
                            options.auto_mode,
                            options.placement_mode,
                            options.protection_mode,
                        )
                        .with_line_number(line_number);

                    let diagnostic = options
                        .diagnostic
                        .map(|kind| entry_diagnostic(line_number, line, kind));

                    (entry, diagnostic, None)
                }
                (None, _, _) => (
                    malformed_nameless_entry(location, line_number),
                    None,
                    Some(format!(
                        "warning: malformed entry at line {}: location must be wrapped in single quotes",
                        line_number
                    )),
                ),
                (_, None, _) => (
                    malformed_nameless_entry(location, line_number),
                    None,
                    Some(format!(
                        "warning: malformed entry at line {}: name must be wrapped in '[' and ']'",
                        line_number
                    )),
                ),
                (_, _, EntryOptionsParseResult::Malformed) => (
                    malformed_nameless_entry(location, line_number),
                    None,
                    Some(format!(
                        "warning: malformed entry at line {}: options must be wrapped in '(' and ')'",
                        line_number
                    )),
                ),
            }
        }
        [location] => (
            malformed_nameless_entry(location, line_number),
            None,
            Some(format!(
                "warning: malformed entry at line {}: missing required name field",
                line_number
            )),
        ),
        _ => (
            malformed_nameless_entry(trimmed, line_number),
            None,
            Some(format!(
                "warning: malformed entry at line {}: expected '<location>' [<name>] (<options>) (or omit options to default to auto/post)",
                line_number
            )),
        ),
    };

    Some(parsed)
}

#[cfg(test)]
fn parse_entry_line(line: &str, line_number: usize) -> Option<PathEntry> {
    parse_entry_line_with_diagnostic(line, line_number).map(|(entry, _, _)| entry)
}

/// Serialize an entry back into a line.
fn format_entry_line(entry: &PathEntry) -> String {
    format!(
        "{} [{}] ({})",
        quote_store_location_field(&entry.location),
        entry.name,
        format_entry_options(entry)
    )
}

#[derive(Debug, Default)]
struct LoadedEntries {
    entries: Vec<PathEntry>,
    diagnostics: Vec<EntryDiagnostic>,
    parse_warnings: Vec<String>,
}

/// Load entries and parser diagnostics from the store file.
fn load_entries_with_diagnostics(store_file: &Path) -> io::Result<LoadedEntries> {
    if !store_file.exists() {
        return Ok(LoadedEntries::default());
    }

    let file = fs::File::open(store_file)?;
    let reader = io::BufReader::new(file);
    let mut loaded = LoadedEntries::default();

    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if let Some((entry, diagnostic, parse_warning)) =
            parse_entry_line_with_diagnostic(&line, index + 1)
        {
            loaded.entries.push(entry);
            if let Some(diagnostic) = diagnostic {
                loaded.diagnostics.push(diagnostic);
            }
            if let Some(parse_warning) = parse_warning {
                loaded.parse_warnings.push(parse_warning);
            }
        }
    }

    Ok(loaded)
}

/// Load entries from the store file; if the file doesn't exist return an empty
/// vector.
fn load_entries(store_file: &Path) -> io::Result<Vec<PathEntry>> {
    Ok(load_entries_with_diagnostics(store_file)?.entries)
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

fn report_invalid_option(level: &str, store_file: &Path, diagnostic: &EntryDiagnostic) {
    let invalid_option = match &diagnostic.kind {
        EntryDiagnosticKind::UnknownOption { option } => option.as_str(),
        _ => "<unknown>",
    };
    eprintln!(
        "{}: unknown entry option '{}' at line {} in {}",
        level,
        invalid_option,
        diagnostic.line_number,
        store_file.display()
    );
    eprintln!("{}: {}", level, sanitize_for_display(&diagnostic.line));
}

fn report_conflicting_options(store_file: &Path, diagnostic: &EntryDiagnostic) {
    let (left, right) = match &diagnostic.kind {
        EntryDiagnosticKind::ConflictingOptions { left, right } => (left.as_str(), right.as_str()),
        _ => ("<unknown>", "<unknown>"),
    };
    eprintln!(
        "error: conflicting entry options '{}' and '{}' at line {} in {}",
        left,
        right,
        diagnostic.line_number,
        store_file.display()
    );
    eprintln!("error: {}", sanitize_for_display(&diagnostic.line));
}

// Collects parse warning messages into a vector for later reporting.
fn collect_parse_warning_messages(parse_warnings: &[String]) -> Vec<String> {
    parse_warnings.to_vec()
}

// Gathers non-fatal store issues into a vector of user-facing warning messages.
fn collect_store_issue_messages_nonfatal(
    store_file: &Path,
    entries: &[PathEntry],
    diagnostics: &[EntryDiagnostic],
) -> Vec<String> {
    let mut messages = Vec::new();

    for diagnostic in diagnostics {
        match &diagnostic.kind {
            EntryDiagnosticKind::UnknownOption { option } => {
                messages.push(format!(
                    "warning: unknown entry option '{}' at line {} in {}",
                    option,
                    diagnostic.line_number,
                    store_file.display()
                ));
                messages.push(format!(
                    "warning: {}",
                    sanitize_for_display(&diagnostic.line)
                ));
            }
            EntryDiagnosticKind::ConflictingOptions { left, right } => {
                messages.push(format!(
                    "error: conflicting entry options '{}' and '{}' at line {} in {}",
                    left,
                    right,
                    diagnostic.line_number,
                    store_file.display()
                ));
                messages.push(format!("error: {}", sanitize_for_display(&diagnostic.line)));
            }
        }
    }

    if let Some(e) = entries.iter().find(|e| e.name.is_empty()) {
        messages.push(format!(
            "error: found nameless entry in {} at line {}: '{}'",
            store_file.display(),
            e.line_number,
            sanitize_for_display(&e.location)
        ));
    }

    if let Some(e) = entries
        .iter()
        .find(|e| !is_store_location_canonical_like(&e.location))
    {
        messages.push(format!(
            "error: invalid stored location '{}' at line {}: locations in {} must be absolute, canonical-looking, and must not contain ':'",
            sanitize_for_display(&e.location),
            e.line_number,
            store_file.display()
        ));
    }

    if let Some(e) = entries.iter().find(|e| !is_valid_name(&e.name)) {
        messages.push(format!(
            "error: invalid name '{}' at line {}: names must contain only alphanumeric characters",
            sanitize_for_display(&e.name),
            e.line_number
        ));
    }

    if let Some(e) = entries
        .iter()
        .find(|e| find_system_path_by_name(&e.name).is_some())
    {
        messages.push(format!(
            "error: name '{}' at line {} is reserved for a protected system path",
            sanitize_for_display(&e.name),
            e.line_number
        ));
    }

    let mut seen_names = std::collections::HashMap::new();
    for e in entries {
        seen_names
            .entry(&e.name)
            .or_insert_with(Vec::new)
            .push(e.line_number);
    }
    for (name, lines) in seen_names.iter().filter(|(_, lines)| lines.len() > 1) {
        let line_list = lines
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        messages.push(format!(
            "error: duplicate name '{}' found at lines: {}",
            sanitize_for_display(name),
            line_list
        ));
    }

    let mut seen_locations = std::collections::HashMap::new();
    for entry in entries {
        seen_locations
            .entry(&entry.location)
            .or_insert_with(Vec::new)
            .push(entry.line_number);
    }
    for (location, lines) in seen_locations.iter().filter(|(_, lines)| lines.len() > 1) {
        let line_list = lines
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        messages.push(format!(
            "error: duplicate path '{}' found at lines: {}",
            sanitize_for_display(location),
            line_list
        ));
    }

    let invalid: Vec<&PathEntry> = entries
        .iter()
        .filter(|e| !Path::new(&e.location).exists())
        .collect();
    if !invalid.is_empty() {
        messages.push("warning: the following stored paths do not exist:".to_string());
        for e in invalid {
            messages.push(format!("  {}", sanitize_for_display(&e.location)));
        }
    }

    messages
}

fn report_parse_warnings(parse_warnings: &[String]) {
    for warning in collect_parse_warning_messages(parse_warnings) {
        eprintln!("{}", warning);
    }
}

fn report_store_issues_nonfatal(
    store_file: &Path,
    entries: &[PathEntry],
    diagnostics: &[EntryDiagnostic],
) {
    for message in collect_store_issue_messages_nonfatal(store_file, entries, diagnostics) {
        eprintln!("{}", message);
    }
}

/// Check the existing entries, reporting any whose `location` does not
/// currently exist. Nameless/duplicate names are fatal errors; missing
/// locations are warnings only. Unknown option handling is controlled by the
/// caller.
fn validate_loaded_entries(
    store_file: &Path,
    entries: &[PathEntry],
    diagnostics: &[EntryDiagnostic],
    fail_on_invalid_option: bool,
) {
    let conflicting_entries: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.kind,
                EntryDiagnosticKind::ConflictingOptions { .. }
            )
        })
        .collect();
    if !conflicting_entries.is_empty() {
        for diagnostic in conflicting_entries {
            report_conflicting_options(store_file, diagnostic);
        }
        std::process::exit(1);
    }

    let mut invalid_entries = diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.kind, EntryDiagnosticKind::UnknownOption { .. }))
        .peekable();
    if invalid_entries.peek().is_some() {
        for diagnostic in invalid_entries {
            report_invalid_option(
                if fail_on_invalid_option {
                    "error"
                } else {
                    "warning"
                },
                store_file,
                diagnostic,
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
            sanitize_for_display(&e.location)
        );
        std::process::exit(1);
    }

    if let Some(e) = entries
        .iter()
        .find(|e| !is_store_location_canonical_like(&e.location))
    {
        eprintln!(
            "error: invalid stored location '{}' at line {}: locations in {} must be absolute, canonical-looking, and must not contain ':'",
            sanitize_for_display(&e.location),
            e.line_number,
            store_file.display()
        );
        std::process::exit(1);
    }

    if let Some(e) = entries.iter().find(|e| !is_valid_name(&e.name)) {
        eprintln!(
            "error: invalid name '{}' at line {}: names must contain only alphanumeric characters",
            sanitize_for_display(&e.name),
            e.line_number
        );
        std::process::exit(1);
    }

    if let Some(e) = entries
        .iter()
        .find(|e| find_system_path_by_name(&e.name).is_some())
    {
        eprintln!(
            "error: name '{}' at line {} is reserved for a protected system path",
            sanitize_for_display(&e.name),
            e.line_number
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
                sanitize_for_display(name),
                line_list
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
                sanitize_for_display(location),
                line_list
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
        eprintln!("  {}", sanitize_for_display(&e.location));
    }
}

/// Validate the configured store file entries.
///
/// Returns an I/O error only if loading entries fails.
fn validate_entries(store_file: &Path) -> io::Result<()> {
    let loaded = load_entries_with_diagnostics(store_file)?;
    report_parse_warnings(&loaded.parse_warnings);
    validate_loaded_entries(store_file, &loaded.entries, &loaded.diagnostics, false);
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
            SubCommand::with_name("list").about("List entries stored in the configured store file"),
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
        sanitize_for_display(&entry.location),
        sanitize_for_display(&entry.name),
        format_entry_options(entry)
    )
}

/// Print the current PATH as a formatted table with index, name, and type columns.
fn print_pretty_path_output(current: &str, entries: &[PathEntry]) {
    let segments: Vec<&str> = if current.is_empty() {
        Vec::new()
    } else {
        current.split(':').collect()
    };

    let names: Vec<String> = segments
        .iter()
        .map(|seg| resolve_segment_name(seg, entries))
        .collect();

    let types: Vec<String> = segments
        .iter()
        .map(|seg| resolve_segment_type(seg, entries))
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
        "{:<index_width$}  {:<path_width$}  {:<name_width$}  TYPE",
        "#",
        "PATH",
        "NAME",
        index_width = index_col_width,
        path_width = path_col_width,
        name_width = name_col_width
    );
    println!(
        "{:-<index_width$}  {:-<path_width$}  {:-<name_width$}  {:-<type_width$}",
        "",
        "",
        "",
        "",
        index_width = index_col_width,
        path_width = path_col_width,
        name_width = name_col_width,
        type_width = type_col_width
    );

    for (index, ((segment, entry_type), name)) in segments
        .iter()
        .zip(types.iter())
        .zip(names.iter())
        .enumerate()
    {
        println!(
            "{:<index_width$}  {:<path_width$}  {:<name_width$}  {}",
            index + 1,
            segment,
            name,
            entry_type,
            index_width = index_col_width,
            path_width = path_col_width,
            name_width = name_col_width
        );
    }
}

/// Handle the `add` subcommand.
///
/// This resolves named entries, validates path shape, optionally persists a
/// named mapping, and prints the resulting PATH export command.
fn handle_add(add_matches: &ArgMatches, store_file: &Path) {
    let mut location = add_matches.value_of("location").unwrap().to_string();
    let mut resolved_by_name = false;
    let placement = if add_matches.is_present("pre") {
        PlacementMode::Prefix
    } else {
        PlacementMode::Postfix
    };

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
    let auto_mode = if add_matches.is_present("noauto") {
        AutosetMode::NoAuto
    } else {
        AutosetMode::Auto
    };
    let protection_mode = if add_matches.is_present("protect") {
        ProtectionMode::Protected
    } else {
        ProtectionMode::Unprotected
    };

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

            entries.push(PathEntry::new(location.clone(), name).with_options(
                auto_mode,
                placement,
                protection_mode,
            ));

            if let Err(error) = save_entries(store_file, &entries) {
                eprintln!("warning: failed to update store file: {}", error);
            }
        }
    }

    let current = env::var("PATH").unwrap_or_default();

    let should_add = !path_contains_segment(&current, &location);

    let updated = if should_add {
        compose_path(&current, &location, placement == PlacementMode::Prefix)
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
            if entry.is_protected() && !force_remove {
                eprintln!(
                    "error: entry '{}' is protected and cannot be removed with 'path remove'",
                    argument
                );
                std::process::exit(1);
            }

            location_to_remove = entry.location.clone();

            if let Some(system_entry) =
                find_system_path_by_location(&location_to_remove).filter(|_| !force_remove)
            {
                eprintln!(
                    "error: system path '{}' ({}) is protected and cannot be removed with 'path remove'",
                    system_entry.location, system_entry.name
                );
                std::process::exit(1);
            }

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
                entry.is_protected() && entry.location == location_to_remove && !force_remove
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
fn handle_list(store_file: &Path) {
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

/// Print the current PATH value as a formatted table.
fn print_pretty_current_path(store_file: &Path) {
    let current = env::var("PATH").unwrap_or_default();
    if !store_file.exists() {
        print_pretty_path_output(&current, &[]);
        eprintln!(
            "warning: store file does not exist: {}",
            store_file.display()
        );
        return;
    }

    let loaded = match load_entries_with_diagnostics(store_file) {
        Ok(loaded) => loaded,
        Err(error) => {
            print_pretty_path_output(&current, &[]);
            eprintln!(
                "warning: failed to load store file for pretty PATH output: {}",
                error
            );
            return;
        }
    };

    print_pretty_path_output(&current, &loaded.entries);
    report_parse_warnings(&loaded.parse_warnings);
    report_store_issues_nonfatal(store_file, &loaded.entries, &loaded.diagnostics);
}

/// Handle the `load` subcommand.
///
/// This applies all entries marked `auto` in the configured store file to PATH,
/// prepending entries marked `pre` and appending all others, while skipping
/// entries already present as exact path segments.
fn handle_load(store_file: &Path) {
    let entries = load_entries_or_exit(store_file);

    let mut current = env::var("PATH").unwrap_or_default();
    for entry in entries.into_iter().filter(|entry| entry.autoset_enabled()) {
        if !path_contains_segment(&current, &entry.location) {
            current = compose_path(&current, &entry.location, entry.prepends_path());
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

    let loaded = match load_entries_with_diagnostics(store_file) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("error: failed to load entries: {}", error);
            std::process::exit(1);
        }
    };

    if loaded.entries.is_empty() {
        eprintln!("error: store file has no entries: {}", store_file.display());
        std::process::exit(1);
    }

    report_parse_warnings(&loaded.parse_warnings);
    validate_loaded_entries(store_file, &loaded.entries, &loaded.diagnostics, true);
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
        .find(|e| !e.name.is_empty() && strip_trailing_slash(&e.location) == normalized)
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
        if system.is_protected() {
            return "system [protected]".to_string();
        }
        return "system".to_string();
    }

    if find_extra_path_by_location(&normalized).is_some() {
        return "known".to_string();
    }

    if entries
        .iter()
        .any(|entry| strip_trailing_slash(&entry.location) == normalized && entry.is_protected())
    {
        return "[protected]".to_string();
    }

    String::new()
}

/// Program entry point.
///
/// Validates stored entries for subcommands that need them, dispatches subcommands,
/// and falls back to pretty-printing the current PATH when no subcommand is provided.
fn main() {
    let matches = build_cli().get_matches();
    let store_file = resolve_store_file_path(&matches);

    if matches.subcommand_matches("verify").is_some() {
        handle_verify(&store_file);
        return;
    }

    // Subcommands that require pre-validation of store entries: add, remove,
    // delete, list, and load.
    // Store validation is intentionally skipped for restore and default pretty
    // output. Default pretty output still reads the store for name/type
    // resolution, but does so with load_entries_or_warn so malformed/unreadable
    // store files do not block PATH output.
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

    if matches.subcommand_matches("list").is_some() {
        handle_list(&store_file);
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

    print_pretty_current_path(&store_file);
}

/// Return `true` for invisible Unicode code points that are not caught by
/// `char::is_control` but can still manipulate terminal rendering or hide
/// content from the viewer (bidirectional overrides, zero-width characters,
/// BOM, and similar format characters).
fn is_invisible_unicode(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'               // Soft hyphen
        | '\u{200B}'..='\u{200D}' // Zero-width space / non-joiner / joiner
        | '\u{200E}' | '\u{200F}' // LTR mark / RTL mark
        | '\u{202A}'..='\u{202E}' // Directional embedding/override characters
        | '\u{2060}'..='\u{2064}' // Word joiner and invisible operators
        | '\u{2066}'..='\u{206F}' // Directional isolates and deprecated format chars
        | '\u{FEFF}'              // BOM / zero-width no-break space
        | '\u{FFF9}'..='\u{FFFB}' // Interlinear annotation characters
    )
}

/// Replace every control character and invisible Unicode code point in `value`
/// with its `\u{XXXX}` escape sequence so that the output is safe to display
/// in a terminal without triggering ANSI sequences, audible bells, cursor
/// movement, bidirectional overrides, or other side-effects.
fn sanitize_for_display(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_control() || is_invisible_unicode(c) {
            use std::fmt::Write as _;
            let _ = write!(result, "\\u{{{:04X}}}", c as u32);
        } else {
            result.push(c);
        }
    }
    result
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
        assert_eq!(entry.auto_mode, AutosetMode::Auto);
        assert_eq!(entry.placement_mode, PlacementMode::Postfix);
        assert_eq!(entry.protection_mode, ProtectionMode::Unprotected);
        assert_eq!(entry.line_number, 0);
    }

    #[test]
    /// Ensure `with_options` updates only the explicit option flags.
    fn path_entry_with_options_updates_option_flags() {
        let entry = PathEntry::new("/tmp/tools", "tools").with_options(
            AutosetMode::NoAuto,
            PlacementMode::Prefix,
            ProtectionMode::Protected,
        );

        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "tools");
        assert!(!entry.autoset_enabled());
        assert!(entry.prepends_path());
        assert!(entry.is_protected());
        assert_eq!(entry.line_number, 0);
    }

    #[test]
    /// Ensure `with_line_number` records source line metadata.
    fn path_entry_with_line_number_updates_line_metadata() {
        let entry = PathEntry::new("/tmp/tools", "tools").with_line_number(4);

        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "tools");
        assert!(entry.autoset_enabled());
        assert!(!entry.prepends_path());
        assert!(!entry.is_protected());
        assert_eq!(entry.line_number, 4);
    }

    #[test]
    /// Ensure default parsed options helper returns default option modes without diagnostics.
    fn default_parsed_options_result_without_diagnostic_uses_defaults() {
        let parsed = default_parsed_options_result(None);

        match parsed {
            EntryOptionsParseResult::Parsed(options) => {
                assert_eq!(options.auto_mode, AutosetMode::Auto);
                assert_eq!(options.placement_mode, PlacementMode::Postfix);
                assert_eq!(options.protection_mode, ProtectionMode::Unprotected);
                assert!(options.diagnostic.is_none());
            }
            EntryOptionsParseResult::Malformed => {
                panic!("expected parsed default option result");
            }
        }
    }

    #[test]
    /// Ensure default parsed options helper preserves a supplied diagnostic.
    fn default_parsed_options_result_with_diagnostic_preserves_diagnostic() {
        let parsed = default_parsed_options_result(Some(EntryDiagnosticKind::ConflictingOptions {
            left: "auto".to_string(),
            right: "noauto".to_string(),
        }));

        match parsed {
            EntryOptionsParseResult::Parsed(options) => {
                assert_eq!(options.auto_mode, AutosetMode::Auto);
                assert_eq!(options.placement_mode, PlacementMode::Postfix);
                assert_eq!(options.protection_mode, ProtectionMode::Unprotected);
                assert_eq!(
                    options.diagnostic,
                    Some(EntryDiagnosticKind::ConflictingOptions {
                        left: "auto".to_string(),
                        right: "noauto".to_string(),
                    })
                );
            }
            EntryOptionsParseResult::Malformed => {
                panic!("expected parsed default option result");
            }
        }
    }

    #[test]
    /// Ensure unknown options are reported as diagnostics separate from `PathEntry`.
    fn parse_entry_line_reports_unknown_option_diagnostic() {
        let line = "'/tmp/tools' [tools] (noauto,pre,protect,postfix)";
        let (entry, diagnostic, parse_warning) = parse_entry_line_with_diagnostic(line, 2).unwrap();

        assert_eq!(entry.name, "tools");
        assert!(!entry.autoset_enabled());
        assert!(entry.prepends_path());
        assert!(entry.is_protected());

        let diagnostic = diagnostic.expect("expected unknown option diagnostic");
        assert!(parse_warning.is_none());
        assert_eq!(diagnostic.line_number, 2);
        assert_eq!(diagnostic.line, line);
        assert_eq!(
            diagnostic.kind,
            EntryDiagnosticKind::UnknownOption {
                option: "postfix".to_string()
            }
        );
    }

    #[test]
    /// Ensure mutually-exclusive options are reported as diagnostics.
    fn parse_entry_line_reports_conflicting_options_diagnostic() {
        let line = "'/tmp/tools' [tools] (auto,noauto)";
        let (entry, diagnostic, parse_warning) = parse_entry_line_with_diagnostic(line, 9).unwrap();

        assert_eq!(entry.name, "tools");

        let diagnostic = diagnostic.expect("expected conflicting options diagnostic");
        assert!(parse_warning.is_none());
        assert_eq!(diagnostic.line_number, 9);
        assert_eq!(diagnostic.line, line);
        assert_eq!(
            diagnostic.kind,
            EntryDiagnosticKind::ConflictingOptions {
                left: "auto".to_string(),
                right: "noauto".to_string(),
            }
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
        assert!(!entry.autoset_enabled());
        assert!(!entry.prepends_path());
        assert!(!entry.is_protected());
        assert_eq!(entry.line_number, 7);
    }

    #[test]
    /// Ensure unwrapped legacy field formats are rejected.
    fn parse_entry_line_legacy_unwrapped_fields_are_rejected() {
        let entry = parse_entry_line("/tmp/tools\ttools\tnoauto", 8).unwrap();
        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "");
        assert!(entry.autoset_enabled());
        assert!(!entry.prepends_path());
        assert!(!entry.is_protected());
    }

    #[test]
    /// Verify lines without a name field become nameless entries for validation.
    fn parse_entry_line_without_name_field_creates_nameless_entry() {
        let entry = parse_entry_line("/tmp/tools", 3).unwrap();
        assert_eq!(entry.location, "/tmp/tools");
        assert_eq!(entry.name, "");
        assert!(entry.autoset_enabled());
        assert!(!entry.prepends_path());
        assert!(!entry.is_protected());
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
        assert!(extras
            .iter()
            .all(|e| e.protection_mode == ProtectionMode::Unprotected));
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
            entry.auto_mode == AutosetMode::Auto
                && entry.placement_mode == PlacementMode::Postfix
                && entry.protection_mode == ProtectionMode::Protected
        }));

        let extras = known_extra_paths();
        assert!(extras.iter().all(|entry| {
            entry.auto_mode == AutosetMode::Auto
                && entry.placement_mode == PlacementMode::Postfix
                && entry.protection_mode == ProtectionMode::Unprotected
        }));
    }

    #[test]
    /// Ensure find_extra_path_by_location returns the matching entry and None for unknowns.
    fn find_extra_path_by_location_returns_correct_entry() {
        let entry = find_extra_path_by_location("/opt/homebrew/bin").unwrap();
        assert_eq!(entry.name, "homebrewbin");
        assert!(!entry.is_protected());

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
            auto_mode: AutosetMode::Auto,
            placement_mode: PlacementMode::Postfix,
            protection_mode: ProtectionMode::Protected,
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
        same.auto_mode = AutosetMode::NoAuto;
        assert_eq!(format_list_entry(&same), "/usr/bin [/usr/bin] (noauto)");

        let mut pre = test_entry("/opt/pre", "pre");
        pre.placement_mode = PlacementMode::Prefix;
        assert_eq!(format_list_entry(&pre), "/opt/pre [pre] (auto,pre)");

        let mut protect = test_entry("/opt/protect", "protect");
        protect.protection_mode = ProtectionMode::Protected;
        assert_eq!(
            format_list_entry(&protect),
            "/opt/protect [protect] (auto,protect)"
        );
    }

    #[test]
    /// Ensure missing third fields are interpreted as `auto` for manual edits.
    fn parse_entry_line_missing_third_field_defaults_to_auto() {
        let entry = parse_entry_line("'/tmp/tools' [tools]", 2).unwrap();
        assert!(entry.autoset_enabled());
        assert!(!entry.prepends_path());
    }

    #[test]
    /// Ensure `(pre)` enables prepend while defaulting autoset to `auto`.
    fn parse_entry_line_pre_option_enables_prepend() {
        let entry = parse_entry_line("'/tmp/tools' [tools] (pre)", 2).unwrap();
        assert!(entry.autoset_enabled());
        assert!(entry.prepends_path());
        assert!(!entry.is_protected());
    }

    #[test]
    /// Ensure comma-delimited options parse both autoset and prepend settings.
    fn parse_entry_line_combined_options_parse() {
        let entry = parse_entry_line("'/tmp/tools' [tools] (noauto,pre)", 2).unwrap();
        assert!(!entry.autoset_enabled());
        assert!(entry.prepends_path());
        assert!(!entry.is_protected());
    }

    #[test]
    /// Ensure `protect` is parsed as a supported option.
    fn parse_entry_line_protect_option_enables_protection() {
        let entry = parse_entry_line("'/tmp/tools' [tools] (auto,protect)", 2).unwrap();
        assert!(entry.autoset_enabled());
        assert!(!entry.prepends_path());
        assert!(entry.is_protected());
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
        let (entry, diagnostic, parse_warning) = parse_entry_line_with_diagnostic(line, 2).unwrap();
        assert_eq!(entry.name, "tools");
        assert!(!entry.autoset_enabled());
        assert!(entry.prepends_path());
        assert!(entry.is_protected());
        assert!(parse_warning.is_none());

        let diagnostic = diagnostic.expect("expected unknown option diagnostic");
        assert_eq!(
            diagnostic.kind,
            EntryDiagnosticKind::UnknownOption {
                option: "postfix".to_string()
            }
        );
        assert_eq!(
            format_entry_line(&entry),
            "'/tmp/tools' [tools] (noauto,pre,protect)"
        );
    }

    #[test]
    /// Ensure quoted locations preserve literal spaces in stored paths.
    fn parse_entry_line_quoted_location_preserves_spaces() {
        let entry = parse_entry_line("'/tmp/my tools' [tools] (auto)", 4).unwrap();
        assert_eq!(entry.location, "/tmp/my tools");
        assert_eq!(entry.name, "tools");
        assert!(entry.autoset_enabled());
        assert!(!entry.prepends_path());
        assert!(!entry.is_protected());
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
        assert!(entry.autoset_enabled());
    }

    #[test]
    /// Ensure entry serialization writes explicit `auto` and `noauto` markers.
    fn format_entry_line_writes_auto_markers() {
        let auto = test_entry("/tmp/a", "a");
        assert_eq!(format_entry_line(&auto), "'/tmp/a' [a] (auto)");

        let mut noauto = test_entry("/tmp/b", "b");
        noauto.auto_mode = AutosetMode::NoAuto;
        assert_eq!(format_entry_line(&noauto), "'/tmp/b' [b] (noauto)");

        let mut pre = test_entry("/tmp/c", "c");
        pre.placement_mode = PlacementMode::Prefix;
        assert_eq!(format_entry_line(&pre), "'/tmp/c' [c] (auto,pre)");

        let mut protect = test_entry("/tmp/d", "d");
        protect.protection_mode = ProtectionMode::Protected;
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

    #[test]
    /// Ensure ParsedOptions can be constructed directly.
    fn parsed_options_can_be_constructed() {
        let options = ParsedOptions {
            auto_mode: AutosetMode::NoAuto,
            placement_mode: PlacementMode::Prefix,
            protection_mode: ProtectionMode::Protected,
            diagnostic: Some(EntryDiagnosticKind::UnknownOption {
                option: "badopt".to_string(),
            }),
        };

        assert_eq!(options.auto_mode, AutosetMode::NoAuto);
        assert_eq!(options.placement_mode, PlacementMode::Prefix);
        assert_eq!(options.protection_mode, ProtectionMode::Protected);
        assert!(options.diagnostic.is_some());
    }

    #[test]
    /// Ensure ParsedOptions can be extracted from Parsed variant.
    fn parsed_options_extracted_from_variant() {
        let parsed = default_parsed_options_result(Some(EntryDiagnosticKind::UnknownOption {
            option: "unknown".to_string(),
        }));

        match parsed {
            EntryOptionsParseResult::Parsed(options) => {
                assert_eq!(options.auto_mode, AutosetMode::Auto);
                assert_eq!(options.placement_mode, PlacementMode::Postfix);
                assert_eq!(options.protection_mode, ProtectionMode::Unprotected);
                assert!(options.diagnostic.is_some());
            }
            EntryOptionsParseResult::Malformed => {
                panic!("expected parsed options result");
            }
        }
    }

    #[test]
    /// Ensure entry_diagnostic helper wraps diagnostic kind with line metadata.
    fn entry_diagnostic_wraps_kind_with_metadata() {
        let kind = EntryDiagnosticKind::UnknownOption {
            option: "badopt".to_string(),
        };
        let diagnostic = entry_diagnostic(5, "'/tmp/test' [test]", kind.clone());

        assert_eq!(diagnostic.line_number, 5);
        assert_eq!(diagnostic.line, "'/tmp/test' [test]");
        assert_eq!(diagnostic.kind, kind);
    }

    #[test]
    /// Ensure entry_diagnostic helper works with conflicting options diagnostic.
    fn entry_diagnostic_wraps_conflicting_options() {
        let kind = EntryDiagnosticKind::ConflictingOptions {
            left: "auto".to_string(),
            right: "noauto".to_string(),
        };
        let diagnostic = entry_diagnostic(10, "'/tmp/conflict' [c] (auto,noauto)", kind.clone());

        assert_eq!(diagnostic.line_number, 10);
        assert_eq!(diagnostic.line, "'/tmp/conflict' [c] (auto,noauto)");
        assert_eq!(diagnostic.kind, kind);
    }

    #[test]
    /// Ensure parse warning collection preserves message order.
    fn collect_parse_warning_messages_preserves_order() {
        let warnings = vec![
            "warning: first parse issue".to_string(),
            "warning: second parse issue".to_string(),
        ];

        assert_eq!(collect_parse_warning_messages(&warnings), warnings);
    }

    #[test]
    /// Ensure non-fatal issue collection includes diagnostics and validation issues.
    fn collect_store_issue_messages_nonfatal_includes_expected_messages() {
        let store_file = Path::new("/tmp/test.path");
        let diagnostics = vec![entry_diagnostic(
            2,
            "'/tmp/tools' [tools] (auto,autoo)",
            EntryDiagnosticKind::UnknownOption {
                option: "autoo".to_string(),
            },
        )];
        let entries = vec![PathEntry::new("/tmp:bad", "tools").with_line_number(2)];

        let messages = collect_store_issue_messages_nonfatal(store_file, &entries, &diagnostics);

        assert!(messages
            .iter()
            .any(|m| m.contains("warning: unknown entry option 'autoo'")));
        assert!(messages
            .iter()
            .any(|m| m.contains("error: invalid stored location '/tmp:bad'")));
    }

    #[test]
    /// Normal printable ASCII passes through `sanitize_for_display` unchanged.
    fn sanitize_for_display_returns_normal_ascii_unchanged() {
        assert_eq!(sanitize_for_display("hello /tmp/path"), "hello /tmp/path");
    }

    #[test]
    /// ESC character (0x1B) is replaced with its Unicode escape representation.
    fn sanitize_for_display_escapes_ansi_escape_sequence() {
        assert_eq!(sanitize_for_display("\x1b[31m"), "\\u{001B}[31m");
    }

    #[test]
    /// BEL character (0x07) is replaced with its Unicode escape representation.
    fn sanitize_for_display_escapes_bell_character() {
        assert_eq!(sanitize_for_display("\x07"), "\\u{0007}");
    }

    #[test]
    /// NUL byte (0x00) is replaced with its Unicode escape representation.
    fn sanitize_for_display_escapes_null_byte() {
        assert_eq!(sanitize_for_display("\x00"), "\\u{0000}");
    }

    #[test]
    /// Carriage return (0x0D) is replaced with its Unicode escape representation.
    fn sanitize_for_display_escapes_carriage_return() {
        assert_eq!(sanitize_for_display("\r"), "\\u{000D}");
    }

    #[test]
    /// Right-to-left override (U+202E) is replaced with its Unicode escape representation.
    fn sanitize_for_display_escapes_bidi_override_character() {
        assert_eq!(sanitize_for_display("\u{202E}"), "\\u{202E}");
    }

    #[test]
    /// Zero-width space (U+200B) is replaced with its Unicode escape representation.
    fn sanitize_for_display_escapes_zero_width_space() {
        assert_eq!(sanitize_for_display("\u{200B}"), "\\u{200B}");
    }

    #[test]
    /// Safe chars adjacent to unsafe chars are preserved; only unsafe chars are escaped.
    fn sanitize_for_display_handles_mixed_safe_and_unsafe_content() {
        assert_eq!(sanitize_for_display("/tmp/\x1bpath"), "/tmp/\\u{001B}path");
    }

    #[test]
    /// `is_invisible_unicode` returns true for the RTL override character (U+202E).
    fn is_invisible_unicode_returns_true_for_rtl_override() {
        assert!(is_invisible_unicode('\u{202E}'));
    }

    #[test]
    /// `is_invisible_unicode` returns true for zero-width space (U+200B).
    fn is_invisible_unicode_returns_true_for_zero_width_space() {
        assert!(is_invisible_unicode('\u{200B}'));
    }

    #[test]
    /// `is_invisible_unicode` returns false for ordinary printable characters.
    fn is_invisible_unicode_returns_false_for_printable_ascii() {
        assert!(!is_invisible_unicode('a'));
        assert!(!is_invisible_unicode('/'));
        assert!(!is_invisible_unicode(' '));
        assert!(!is_invisible_unicode('Z'));
    }
}
