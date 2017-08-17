extern crate memmap;
extern crate uuid;

use memmap::Mmap;
use std::io::{StdoutLock, Write};
use std::vec::Vec;

const MAX_BUF_SIZE: usize = 4 * 1024 * 1024; //4MiB

fn version() {
    println!("tac 0.2.3 - Copyright NeoSmart Technologies 2017");
    println!("Report bugs at <https://github.com/neosmart/tac>");
}

fn help() {
    version();
    println!("");
    println!("Usage: tac [OPTIONS] [FILE1..]");
    println!("Write each FILE to standard output, last line first.");
    println!("Reads from STTDIN if no file is specified.");
    println!("");
    println!("Options:");
    println!("  -v --version: Print version and exit.");
    println!("  -h --help   : Print this help text and exit");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files = Vec::new();
    for arg in args.iter().skip(1).map(|s| s.as_str()) {
        match arg {
            "-h" | "--help" => {
                help();
                std::process::exit(0);
            }
            "-v" | "--version" => {
                version();
                std::process::exit(0);
            }
            file => {
                if file.starts_with("-") && file != "-" {
                    eprintln!("Invalid option {}!", file);
                    eprintln!("Try 'tac --help' for more information");
                    std::process::exit(-1);
                }
                files.push(file)
            }
        }
    }

    //read from stdin by default
    if files.len() == 0 {
        files.push("-");
    }

    for file in files {
        if let Err(e) = reverse_file(file) {
            eprintln!("{}: {:?}", file, e);
            std::process::exit(-1);
        }
    }
}

fn print_bytes(mut stdout: &mut StdoutLock, bytes: &[u8]) {
    if stdout.write_all(bytes).is_err() {
        std::process::exit(-1);
    }
}

fn reverse_file(path: &str) -> std::io::Result<()> {
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use uuid::Uuid;

    let mmap;
    let mut line;
    let mut temp_path = PathBuf::new();
    let mut delete_on_exit = false;

    {
        let (file, len) = match path {
            "-" => {
                //read from stdin
                //buffer up to MAX_BUFF_SIZE in memory

                let mut in_mem = true;
                line = String::new();

                {
                    let stdin = std::io::stdin();

                    let mut file = None;
                    while stdin.read_line(&mut line)? != 0 {
                        if in_mem && line.len() > MAX_BUF_SIZE {
                            temp_path = std::env::temp_dir()
                                .join(format!("{}", Uuid::new_v4().hyphenated()));
                            let mut temp_file = File::create(&temp_path)?;

                            //write everything we've read so far
                            temp_file.write_all(line.as_bytes())?;
                            //assign new string and not clear because we'll be using a smaller
                            //buffer now
                            line = String::new();
                            in_mem = false;
                            delete_on_exit = true;
                            file = Some(temp_file);
                        } else if !in_mem {
                            let mut temp_file = file.unwrap();
                            temp_file.write_all(line.as_bytes())?;
                            file = Some(temp_file);
                            line.clear();
                        }
                    }
                }

                match in_mem {
                    true => (line.as_bytes(), line.as_bytes().len()),
                    false => {
                        mmap = Mmap::open_path(&temp_path, memmap::Protection::Read)?;
                        let bytes = unsafe { mmap.as_slice() };
                        (bytes, mmap.len())
                    }
                }
            }
            _ => {
                mmap = Mmap::open_path(path, memmap::Protection::Read)?;
                let bytes = unsafe { mmap.as_slice() };
                (bytes, mmap.len())
            }
        };

        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();

        let mut last_printed = len as i64;
        let mut index = last_printed - 1;
        while index > -2 {
            if index == -1 || file[index as usize] == '\n' as u8 {
                print_bytes(&mut stdout,
                            &file[(index + 1) as usize..last_printed as usize]);
                last_printed = index + 1;
            }

            index -= 1;
        }
    }

    if delete_on_exit {
        //this should never fail unless we've somehow kept a handle open to it
        std::fs::remove_file(&temp_path)
            .expect(&format!("Failed to remove temporary file {}", temp_path.display()));
    }

    return Ok(());
}
