#![feature(core_intrinsics)]

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

// Naïve implementation
// #[inline(always)]
fn naive(bytes: &[u8], output: &mut dyn Write) -> Result<(), std::io::Error> {
    let mut last_printed = bytes.len() as i64;
    let mut index = last_printed - 1;

    while index > -2 {
        if index == -1 || bytes[index as usize] == ('\n' as u8) {
            output.write_all(&bytes[(index + 1) as usize..last_printed as usize])?;
            last_printed = index + 1;
        }

        index -= 1;
    }

    Ok(())
}

#[inline(never)]
pub fn search256(bytes: &[u8], mut output: &mut dyn Write) -> Result<(), std::io::Error> {
    let ptr = bytes.as_ptr();
    let mut last_printed = bytes.len();
    // eprintln!("total length: {}", bytes.len());
    let mut index = last_printed - 1;

    // Search index-by-index a range and print
    let slow_search_and_print = |start: usize,
                                 end: usize,
                                 stop: &mut usize,
                                 output: &mut dyn Write|
     -> Result<(), std::io::Error> {
        // eprintln!("Slow-searching bytes {}-{}", start, end);
        let mut i = end;
        while i > start {
            i -= 1;
            // eprintln!("Checking byte {}", i);
            if bytes[i] == ('\n' as u8) {
                // println!("Printing from {} to {}", i, *stop);
                output.write_all(&bytes[i+1..*stop])?;
                *stop = i + 1;
            }
        }

        Ok(())
    };

    // We can only use 32-byte (256-bit) aligned reads w/ AVX2 intrinsics
    // Search via slow method trailing bytes so subsequent aligned reads are always in the
    // haystack.
    if index >= 32 {
        // Regardless of whether or not the base pointer is aligned to a 32-byte address, we are
        // reading from an arbitrary offset (determined by the length of the lines) and so we must
        // first calculate a safe place to begin using SIMD operations from.
        let align_offset = unsafe { ptr.offset(index as isize).align_offset(32) };
        let aligned_index = index as usize + align_offset - 32;
        debug_assert!(aligned_index <= index as usize && aligned_index < last_printed && aligned_index > 0);
        let base_addr: usize = unsafe { *(std::mem::transmute::<* const _, * const usize>(&ptr)) };
        debug_assert!((base_addr + aligned_index as usize) % 32 == 0, "Adjusted index is still not at 256-bit boundary!");
        slow_search_and_print(aligned_index, last_printed, &mut last_printed, &mut output)?;
        index = aligned_index;
        drop(aligned_index);

        assert!('\n' as i8 as u8 as char == '\n');

        let pattern256 = unsafe { core::arch::x86_64::_mm256_set1_epi8('\n' as i8) };
        // dbg!(index);
        // dbg!(last_printed);
        // dbg!(ptr);
        while index >= 32 {
            let window_end_offset = index;
            index -= 32;
            let window = unsafe { ptr.add(index) };
            #[allow(unused)]
            let index: (); // Prevent inadvertant access to the wrong variable

            // unsafe {
            //     // Debug the contents of the window
            //     let mut window_contents = [' '; 32];
            //     for i in 0..32 {
            //         window_contents[i] = window.add(i).read() as char;
            //     }
            //     dbg!(window_contents);
            // }

            // println!("Aligned search of range {}-{}", window, window + 32);
            // slow_search_and_print(window as usize, window as usize + 32, &mut last_printed, &mut output)?;
            // continue;
            // println!("input: {:?}", &bytes[window as usize..(window as usize + 32)]);
            unsafe {
                // let search256 = core::arch::x86_64::_mm256_load_si256(
                //     window as *const core::arch::x86_64::__m256i,
                // );
                let search256 = core::arch::x86_64::_mm256_loadu_si256(
                    window as *const core::arch::x86_64::__m256i,
                );
                let result256 =
                    core::arch::x86_64::_mm256_cmpeq_epi8(search256, pattern256);
                let mut matches: i32 = core::arch::x86_64::_mm256_movemask_epi8(result256);

                // let mut mask2 = 0;
                // let mut letters = String::new();
                // for i in 0..32 {
                //     let c = window.add(i).read() as char;
                //     letters.push(match c { '\n' => '\u{2424}', '\r' => '\u{240d}', _ => c });
                //     if c == '\n' {
                //         mask2 |= 1 << (32 - i - 1) as usize;
                //     }
                // }
                // print!("\n         actual letters: {}\n", letters);
                // println!("expected matches bitset: {:>0width$b}", mask2, width = 32);
                // println!("         matches bitset: {:>0width$b}", matches, width = 32);

                // The generated result mask is in reverse order.

                // let mut matches2 = String::new();
                // for i in 0..32 {
                //     if matches & (1 << (i)) != 0 {
                //         matches2.push('1');
                //     } else {
                //         matches2.push('0');
                //     }
                // }
                while matches != 0 {
                    // We would count *trailing* zeroes to find new lines in reverse order, but the
                    // result mask is in little endian (reversed) order, so we do the very
                    // opposite.
                    let leading = core::intrinsics::ctlz_nonzero(matches);
                    let offset = window_end_offset - leading as usize;
                    // println!("Found at {} + {}", window, 32 - trailing);
                    // assert_eq!(bytes[offset] as char, '\n');

                    // println!("Printing from {} to {}", offset, last_printed);
                    output.write_all(&bytes[offset..last_printed])?;
                    last_printed = offset;
                    // Clear this match from the matches bitset
                    // matches &= !(1 << trailing);
                    matches &= !(1 << (32 - leading - 1));
                }

                // println!("        matches2 bitset: {0}", matches2);
            }
        }
    }

    // eprintln!("Unoptimized end search beginning at index {}", index);
    if index <= 32 {
        slow_search_and_print(0, index as usize, &mut last_printed, &mut output)?;
    }

    if last_printed != 0 {
        output.write_all(&bytes[0..last_printed])?;
    }

    Ok(())
}

// #[inline(always)]
fn search64(bytes: &[u8], output: &mut dyn Write) -> Result<(), std::io::Error> {
    let mut last_printed = bytes.len();
    let mut index = last_printed as i64 - 9;

    let ptr = bytes.as_ptr();
    let pattern64 = 0xaaaaaaaaaaaaaaaau64;
    while index >= 0 {
        let search64: u64 = unsafe { *(ptr.add(index as usize) as *const u64) };
        // eprintln!("Quick-searching bytes {} through {}", index, index + 8);
        if (search64 & pattern64) != 0 {
            // We could use cctz here to find the last match and go from there
            for i in 0..8 {
                let offset = index as usize + 8 - i;
                // eprintln!("Testing byte {}", offset);
                if bytes[offset] == ('\n' as u8) {
                    // eprintln!("Printing {:?}", &bytes[offset+1..last_printed]);
                    output.write_all(&bytes[offset + 1..last_printed])?;
                    // if first_line {
                    //     if offset != last_printed - 1 {
                    //         output.write_all(&bytes[offset+1..last_printed])?;
                    //         output.write(&['\n' as u8])?;
                    //     }
                    //     first_line = false;
                    // } else {
                    //     output.write_all(&bytes[offset+1..last_printed+1])?;
                    // }
                    last_printed = offset + 1;
                }
            }
        }
        index -= 8;
    }

    index += 8;
    while index >= -1 {
        // eprintln!("Testing byte {}", index);
        if index == -1 || bytes[index as usize] == ('\n' as u8) {
            output.write_all(&bytes[(index + 1) as usize..last_printed])?;
            // if first_line {
            //     if index as usize + 1 != last_printed {
            //         output.write_all(&bytes[(index + 1) as usize..last_printed])?;
            //         output.write(&['\n' as u8])?;
            //     }
            //     first_line = false;
            // } else {
            //     output.write_all(&bytes[(index + 1) as usize..last_printed + 1])?;
            // }

            last_printed = index as usize + 1;
        }
        index -= 1;
    }

    Ok(())
}

fn main() {
    let args = std::env::args();
    let mut files = Vec::new();
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
                                std::env::temp_dir().join(format!(".tac-{}", std::process::id())),
                            );
                            let mut temp_file = File::create(temp_path.as_ref().unwrap())?;

                            // Write everything we've read so far
                            temp_file.write_all(&buf[0..total_read])?;
                            file = Some(temp_file);
                        }
                    } else {
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

        let output = std::io::stdout();
        let mut output = output.lock();
        let mut buffered_output;

        let output: &mut dyn Write = if force_flush || atty::is(atty::Stream::Stdout) {
            &mut output
        } else {
            buffered_output = BufWriter::new(output);
            &mut buffered_output
        };

        // naive(bytes, output)?;
        // search64(bytes, output)?;
        search256(bytes, output)?;
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
