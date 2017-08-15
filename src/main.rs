extern crate memmap;
extern crate uuid;

use memmap::Mmap;
use std::vec::Vec;

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

    //read from stdin by default
    if files.len() == 0 {
        files.push("-");
    }

    for file in files {
        match reverse_file(file) {
            Err(e) => eprintln!("{}", e),
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
    use std::path::PathBuf;
    use uuid::Uuid;

    let mut temp_path = PathBuf::new();

    {
        let path = match path {
            "-" => {
                //read from stdin
                //we have no idea how big this can get, don't buffer in memory
                temp_path = std::env::temp_dir().join(format!("{}", Uuid::new_v4().hyphenated()));
                let mut temp_file = File::create(&temp_path).map_err(to_str)?;
                let stdin = std::io::stdin();

                let mut line = String::new();
                while stdin.read_line(&mut line).map_err(to_str)? != 0 {
                    temp_file.write_all(line.as_bytes()).map_err(to_str)?;
                    line.clear();
                }

                temp_path.to_str().unwrap()
            },
            _ => path
        };

        let file = Mmap::open_path(path, memmap::Protection::Read).map_err(to_str)?;
        let len = file.len();

        let mmap = unsafe {
            file.as_slice()
        };

        let mut last_printed: i64 = len as i64;
        let mut index = last_printed - 1;
        while index > -2 {
            if index == -1 || mmap[index as usize] == '\n' as u8 {
                print_bytes(&mmap[(index + 1) as usize..last_printed as usize]);
                last_printed = index + 1;
            }

            index -= 1;
        }
    }

    if temp_path != PathBuf::new() {
        std::fs::remove_file(&temp_path)
            .map_err(|e| format!("Failed to remove temporary file {}: {}", temp_path.display(), e))?;
    }

    return Ok(());
}
