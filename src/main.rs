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
}

const STORE_FILE: &str = ".path";

/// Parse a line from the store file.  Format is
/// `<location>\t<name>\t<exclusivity?>` where exclusivity is `true` or
/// `false` (absent means `None`).
fn parse_entry_line(line: &str) -> Option<PathEntry> {
    let parts: Vec<&str> = line.splitn(3, '\t').collect();
    if parts.len() < 2 {
        return None;
    }
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
        location: parts[0].to_string(),
        name: parts[1].to_string(),
        exclusivity,
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
    for l in reader.lines().map_while(Result::ok) {
        if let Some(e) = parse_entry_line(&l) {
            entries.push(e);
        }
    }
    Ok(entries)
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
fn main() {
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
        .get_matches();

    if let Some(add_matches) = matches.subcommand_matches("add") {
        let loc = add_matches.value_of("location").unwrap();
        // name may be supplied as second positional argument; default to the
        // location string if none provided
        let name = add_matches
            .value_of("name")
            .map(|s| s.to_string())
            .unwrap_or_else(|| loc.to_string());
        let exclusivity = if add_matches.is_present("exclusive") {
            Some(true)
        } else {
            None
        };

        // persist to store file
        if let Ok(mut entries) = load_entries() {
            entries.push(PathEntry {
                location: loc.to_string(),
                name: name.clone(),
                exclusivity,
            });
            if let Err(e) = save_entries(&entries) {
                eprintln!("warning: failed to update store file: {}", e);
            }
        }

        let prepend = add_matches.is_present("pre");
        let current = env::var("PATH").unwrap_or_default();
        let new_path = if prepend {
            format!("{}:{}", loc, current)
        } else {
            // default to append
            if current.is_empty() {
                loc.to_string()
            } else {
                format!("{}:{}", current, loc)
            }
        };
        println!("{}", new_path);
        return;
    }

    // No subcommand: just print the current PATH
    match env::var("PATH") {
        Ok(path) => println!("{}", path),
        Err(e) => eprintln!("Failed to read PATH: {}", e),
    }
}
