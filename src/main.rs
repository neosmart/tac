// NEON SIMD intrinsics for aarch64 are not yet stabilized and require the nightly compiler
#![cfg_attr(all(feature = "nightly", target_arch = "aarch64"), feature(stdsimd))]

mod tac;

fn version() {
    println!(
        "tac {} - Copyright Mahmoud Al-Qudsi, NeoSmart Technologies 2017-2021",
        env!("CARGO_PKG_VERSION")
    );
    println!("Report bugs at <https://github.com/neosmart/tac>");
}

fn help() {
    version();
    println!("");
    println!("Usage: tac [OPTIONS] [FILE1..]");
    println!("Write each FILE to standard output, last line first.");
    println!("Reads from stdin if FILE is - or not specified.");
    println!("");
    println!("Options:");
    println!("  -h --help        Print this help text and exit");
    println!("  -v --version     Print version and exit.");
    println!("  --line-buffered  Always flush output after each line.");
}

fn main() {
    let args = std::env::args();
    // This is intentionally one more than what we might need, in case no arguments were provided
    // and we want to stub a "-" argument in there.
    let mut files = Vec::with_capacity(args.len());
    let mut force_flush = false;
    let mut skip_switches = false;
    for arg in args.skip(1) {
        if !skip_switches && arg.starts_with("-") && arg != "-" {
            match arg.as_str() {
                "-h" | "--help" => {
                    help();
                    std::process::exit(0);
                }
                "-v" | "--version" => {
                    version();
                    std::process::exit(0);
                }
                "--line-buffered" => {
                    force_flush = true;
                }
                "--" => {
                    skip_switches = true;
                    continue;
                }
                _ => {
                    eprintln!("{}: Invalid option!", arg);
                    eprintln!("Try 'tac --help' for more information");
                    std::process::exit(-1);
                }
            }
        } else {
            let file = arg;
            files.push(file)
        }
    }

    // Read from stdin by default
    if files.len() == 0 {
        files.push("-".into());
    }

    for file in &files {
        if let Err(e) = tac::reverse_file(file, force_flush) {
            eprintln!("{}: {:?}", file, e);
            std::process::exit(-1);
        }
    }
}
