use clap::{App, Arg, SubCommand};
use std::env;

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
                    Arg::with_name("pre")
                        .long("pre")
                        .help("Prepend the location instead of appending")
                        .takes_value(false),
                ),
        )
        .get_matches();

    if let Some(add_matches) = matches.subcommand_matches("add") {
        let loc = add_matches.value_of("location").unwrap();
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
