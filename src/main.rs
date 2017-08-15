use std::fs::File;
use std::io::prelude::*;
use std::vec::Vec;

const BUFSIZE: usize = 1024;

fn version() {
    println!("tac 0.1. Copyright NeoSmart Technologies 2017");
    std::process::exit(0);
}

fn help() {
    println!("tac [-v | -h | FILE1 [FILE2 [FILE3 ...]]]");
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
                if file.starts_with("-") {
                    eprintln!("Usage error: argument {} is not recognized! Run tac --help to see \
                               valid command-line options.",
                              file);
                    std::process::exit(-1);
                }
                files.push(file)
            }
        }
    }

    for file in files {
        match reverse_file(file) {
            Err(e) => eprintln!("{}", e),
            _ => {}
        }
    }
}

fn to_err_str(err: std::io::Error) -> String {
    return format!("{}", err);
}

fn reverse_file(path: &str) -> Result<(), String> {
    use std::io::SeekFrom;

    let len = std::fs::metadata(path).map_err(to_err_str)?.len() as usize;
    let mut file = File::open(path).map_err(to_err_str)?;

    let mut file_read_index: usize = if len > BUFSIZE { (len - BUFSIZE) as usize } else { 0 };
    let mut buffer: [u8; BUFSIZE] = [0u8; BUFSIZE];

    loop {
        file.seek(SeekFrom::Start(file_read_index as u64)).map_err(to_err_str)?;
        let bytes_read = file.read(&mut buffer[..]).map_err(to_err_str)?;
        eprintln!("Read {} bytes", bytes_read);
        let read_view = &buffer[0..bytes_read];
        let mut last_new_line = if bytes_read != 0 { bytes_read - 1 } else { 0 };

        for i in (1..last_new_line + 1).rev() {
            if read_view[i] == 10 {
                let unsafe_str = unsafe { std::str::from_utf8_unchecked(&read_view[i..last_new_line]) };
                println!("{}", unsafe_str);
                last_new_line = i;
            }
        }

        if file_read_index == 0 {
            //everything that's left is one line
            let unsafe_str = unsafe { std::str::from_utf8_unchecked(&read_view[0..last_new_line]) };
            println!("{}", unsafe_str);
            break;
        }
        else {
            file_read_index = if file_read_index < BUFSIZE { 0 } else { file_read_index - BUFSIZE };
        }
    }

    return Ok(());
}
