extern crate atty;
extern crate memmap;

use memmap::Mmap;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufWriter;

const MAX_BUF_SIZE: usize = 4 * 1024 * 1024; // 4 MiB

fn version() {
    println!(
        "tac {} - Copyright NeoSmart Technologies 2017-2019",
        env!("CARGO_PKG_VERSION")
    );
    println!("Developed by Mahmoud Al-Qudsi <mqudsi@neosmart.net>");
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
    let args: Vec<String> = std::env::args().collect();
    let mut files = Vec::new();
    let mut force_flush = false;
    let mut skip_switches = false;
    for arg in args.iter().skip(1).map(|s| s.as_str()) {
        if !skip_switches && arg.starts_with("-") && arg.len() > 1 {
            match arg {
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
        files.push("-");
    }

    for file in files {
        if let Err(e) = reverse_file(file, force_flush) {
            eprintln!("{}: {:?}", file, e);
            std::process::exit(-1);
        }
    }
}

fn reverse_file(path: &str, force_flush: bool) -> std::io::Result<()> {
    let mmap;
    let mut buf;
    let mut temp_path = None;

    {
        let bytes = match path {
            "-" => {
                // We unfortunately need to buffer the entirety of the stdin input first;
                // we try to do so purely in memory but will switch to a backing file if
                // the input exceeds MAX_BUF_SIZE.
                buf = Some(Vec::new());
                let buf = buf.as_mut().unwrap();
                let mut reader = std::io::stdin();
                let mut total_read = 0;

                // Once/if we switch to a file-backed buffer, this will contain the handle.
                let mut file: Option<File> = None;
                buf.resize(MAX_BUF_SIZE, 0);

                loop {
                    let bytes_read = reader.read(&mut buf[total_read..])?;
                    if bytes_read == 0 {
                        break;
                    }

                    total_read += bytes_read;
                    // Here we are using `if`/`else` rather than `match` to support mutating
                    // the `file` variable inside the block under older versions of rust.
                    if file.is_none() {
                        if total_read >= MAX_BUF_SIZE {
                            temp_path = Some(
                                std::env::temp_dir()
                                    .join(format!(".tac-{}", std::process::id())),
                            );
                            let mut temp_file = File::create(temp_path.as_ref().unwrap())?;

                            // Write everything we've read so far
                            temp_file.write_all(&buf[0..total_read])?;
                            file = Some(temp_file);
                        }
                    }
                    else {
                        let temp_file = file.as_mut().unwrap();
                        temp_file.write_all(&buf[0..bytes_read])?;
                    }
                }

                // At this point, we have fully consumed the input and can proceed
                // as if it were a normal source rather than stdin.

                match &file {
                    None => &buf[0..total_read],
                    Some(temp_file) => {
                        mmap = unsafe { Mmap::map(&temp_file)? };
                        &mmap[..]
                    }
                }
            }
            _ => {
                let file = File::open(path)?;
                mmap = unsafe { Mmap::map(&file)? };
                &mmap[..]
            }
        };

        let mut output = std::io::stdout();
        let mut buffered_output;

        let output: &mut dyn Write = if force_flush || atty::is(atty::Stream::Stdout) {
            &mut output
        } else {
            buffered_output = BufWriter::new(output);
            &mut buffered_output
        };

        let mut last_printed = bytes.len() as i64;
        let mut index = last_printed - 1;
        while index > -2 {
            if index == -1 || bytes[index as usize] == ('\n' as u8) {
                output.write_all(&bytes[(index + 1) as usize..last_printed as usize])?;
                last_printed = index + 1;
            }

            index -= 1;
        }
    }

    if let Some(ref path) = temp_path.as_ref() {
        // This should never fail unless we've somehow kept a handle open to it
        if let Err(e) = std::fs::remove_file(&path) {
            eprintln!(
                "Error: failed to remove temporary file {}\n{}",
                path.display(),
                e
            )
        };
    }

    return Ok(());
}
