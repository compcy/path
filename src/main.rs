#![deny(warnings)]

use clap::{App, Arg, SubCommand};
use std::env;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

/// Simple representation of a stored path entry in plain text form.
#[derive(Debug, Clone)]
struct PathEntry {
    location: String,
    name: String,
    exclusivity: Option<bool>,
    line_number: usize,
}

const STORE_FILE: &str = ".path";

/// Parse a line from the store file.  Format is
/// `<location>\t<name>\t<exclusivity?>` where exclusivity is `true` or
/// `false` (absent means `None`).
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

    let exclusivity = if parts.len() == 3 {
        match parts[2].trim() {
            "true" => Some(true),
            "false" => Some(false),
            "" => None,
            other => {
                // ignore malformed value
                eprintln!("warning: invalid exclusivity value '{}', ignoring", other);
                None
            }
        }
    } else {
        None
    };
    Some(PathEntry {
        location,
        name,
        exclusivity,
        line_number,
    })
}

/// Serialize an entry back into a line.
fn format_entry_line(entry: &PathEntry) -> String {
    let excl = match entry.exclusivity {
        Some(true) => "true",
        Some(false) => "false",
        None => "",
    };
    format!("{}\t{}\t{}", entry.location, entry.name, excl)
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
    let mut line_number = 0;
    for l in reader.lines().map_while(Result::ok) {
        line_number += 1;
        if let Some(e) = parse_entry_line(&l, line_number) {
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
/// currently exist.  If invalid entries are found, prints them and prompts
/// the user on stdin to optionally remove them.  Returns an I/O error only if
/// reading/writing fails.
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

fn main() {
    // perform validation on startup; ignore errors other than reporting them
    if let Err(e) = validate_entries() {
        eprintln!("warning: could not validate entries: {}", e);
    }

    let matches = App::new("path")
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
                    Arg::with_name("exclusive")
                        .long("exclusive")
                        .help("Mark this entry as exclusive")
                        .takes_value(false),
                )
                .arg(
                    Arg::with_name("pre")
                        .long("pre")
                        .help("Prepend the location instead of appending")
                        .takes_value(false),
                ),
        )
        .subcommand(SubCommand::with_name("list").about("List entries stored in the .path file"))
        .get_matches();

    if let Some(add_matches) = matches.subcommand_matches("add") {
        let mut loc = add_matches.value_of("location").unwrap().to_string();
        
        // Check if the location argument is actually a stored name in the .path file
        // If so, use the associated path instead
        if let Ok(entries) = load_entries() {
            if let Some(entry) = entries.iter().find(|e| e.name == loc) {
                loc = entry.location.clone();
            }
        }
        
        // warn immediately if the specified location doesn't currently exist
        if !Path::new(&loc).exists() {
            eprintln!("warning: added path '{}' does not exist", loc);
        }
        // name may be supplied as second positional argument; keep track of the
        // optional value separately so we know whether it was provided by the
        // user. If absent we won't write anything to the store.
        let name_opt = add_matches.value_of("name").map(|s| s.to_string());

        let exclusivity = if add_matches.is_present("exclusive") {
            Some(true)
        } else {
            None
        };

        // persist only if a name was explicitly given
        if let Some(name_str) = name_opt {
            // validate name format (alphanumeric only)
            if !is_valid_name(&name_str) {
                eprintln!(
                    "error: invalid name '{}': names must contain only alphanumeric characters",
                    name_str
                );
                std::process::exit(1);
            }
            if let Ok(mut entries) = load_entries() {
                // reject if this name is already in use
                if entries.iter().any(|e| e.name == name_str) {
                    eprintln!("error: name '{}' is already in use", name_str);
                    std::process::exit(1);
                }
                // attempt to turn the location into an absolute path so that
                // later validation works regardless of current working
                // directory.
                let stored_loc = match fs::canonicalize(&loc) {
                    Ok(p) => p.to_string_lossy().into_owned(),
                    Err(_) => {
                        eprintln!(
                            "warning: could not canonicalize path '{}', storing as-is",
                            loc
                        );
                        loc.to_string()
                    }
                };
                entries.push(PathEntry {
                    location: stored_loc,
                    name: name_str,
                    exclusivity,
                    line_number: 0,
                });
                if let Err(e) = save_entries(&entries) {
                    eprintln!("warning: failed to update store file: {}", e);
                }
            }
        }

        let prepend = add_matches.is_present("pre");
        let current = env::var("PATH").unwrap_or_default();
        let new_path = if prepend {
            format!("{}:{}", loc, current)
        } else {
            // default to append
            if current.is_empty() {
                loc
            } else {
                format!("{}:{}", current, loc)
            }
        };
        println!("{}", new_path);
        return;
    }

    if matches.subcommand_matches("list").is_some() {
        // display each stored entry; show name if it's different from location
        if let Ok(entries) = load_entries() {
            for e in entries {
                if e.name != e.location {
                    println!("{} ({})", e.location, e.name);
                } else {
                    println!("{}", e.location);
                }
            }
        }
        return;
    }

    // No subcommand: just print the current PATH
    match env::var("PATH") {
        Ok(path) => println!("{}", path),
        Err(e) => eprintln!("Failed to read PATH: {}", e),
    }
}
