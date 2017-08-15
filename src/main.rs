extern crate ctrlc;
extern crate memmap;
extern crate uuid;

use memmap::Mmap;
use std::vec::Vec;

const MAX_BUF_SIZE: usize = 4 * 1024 * 1024; //4MiB

fn version() {
    println!("tac 0.1 - Copyright NeoSmart Technologies 2017");
    println!("Report bugs at <https://github.com/neosmart/tac>");
    std::process::exit(0);
}

fn help() {
    println!("Usage: tac [OPTIONS] [FILE1..]");
    println!("Write each FILE to standard output, last line first.");
    println!("Reads from STTDIN if no file is specified.");
    println!("");
    println!("Options:");
    println!("  -v --version: Print version and exit.");
    println!("  -h --help   : Print this help text and exit");

    std::process::exit(0);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files = Vec::new();
    for arg in args.iter().skip(1).map(|s| s.as_str()) {
        match arg {
            "-h" | "--help" => help(),
            "-v" | "--version" => version(),
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

    //we should abort on SIGPIPE, but rust doesn't do signal handling yet
    //for now, we can catch SIGINT via the ctrlc crate
    ctrlc::set_handler(|| {
        std::process::exit(-1);
    }).expect("Failed to set SIGINT handler!");

    //read from stdin by default
    if files.len() == 0 {
        files.push("-");
    }

    for file in files {
        match reverse_file(file) {
            Err(e) => panic!("{}", e),
            _ => {}
        }
    }
}

fn to_str(err: std::io::Error) -> String {
    return format!("{}", err);
}

fn print_bytes(bytes: &[u8]) {
    let unsafe_str = unsafe { std::str::from_utf8_unchecked(bytes) };
    print!("{}", unsafe_str);
}

fn reverse_file(path: &str) -> Result<(), String> {
    use std::fs::File;
    use std::io::Write;
    use uuid::Uuid;

    let mmap;
    let mut line;
    let mut temp_path = unsafe { std::mem::uninitialized() };
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

                    let mut temp_file = unsafe { std::mem::uninitialized() };
                    while stdin.read_line(&mut line).map_err(to_str)? != 0 {
                        if in_mem && line.len() > MAX_BUF_SIZE {
                            temp_path = std::env::temp_dir()
                                .join(format!("{}", Uuid::new_v4().hyphenated()));
                            temp_file = File::create(&temp_path).map_err(to_str)?;

                            //write everything we've read so far
                            temp_file.write_all(line.as_bytes()).map_err(to_str)?;
                            //assign new string and not clear because we'll be using a smaller
                            //buffer now
                            line = String::new();
                            eprintln!("Switching to file mode");
                            in_mem = false;
                            delete_on_exit = true;
                        } else if !in_mem {
                            temp_file.write_all(line.as_bytes()).map_err(to_str)?;
                            line.clear();
                        }
                    }
                }

                match in_mem {
                    true => (line.as_bytes(), line.as_bytes().len()),
                    false => {
                        mmap =
                            Mmap::open_path(&temp_path, memmap::Protection::Read).map_err(to_str)?;
                        let bytes = unsafe { mmap.as_slice() };
                        (bytes, mmap.len())
                    }
                }
            }
            _ => {
                mmap = Mmap::open_path(path, memmap::Protection::Read).map_err(to_str)?;
                let bytes = unsafe { mmap.as_slice() };
                (bytes, mmap.len())
            }
        };

        let mut last_printed: i64 = len as i64;
        let mut index = last_printed - 1;
        while index > -2 {
            if index == -1 || file[index as usize] == '\n' as u8 {
                print_bytes(&file[(index + 1) as usize..last_printed as usize]);
                last_printed = index + 1;
            }

            index -= 1;
        }
    }

    if delete_on_exit {
        std::fs::remove_file(&temp_path).map_err(|e| {
                format!("Failed to remove temporary file {}: {}",
                        temp_path.display(),
                        e)
            })?;
    }

    return Ok(());
}
