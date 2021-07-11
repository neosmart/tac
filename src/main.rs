extern crate atty;
extern crate memmap;

use memmap::Mmap;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufWriter;

const SEARCH: u8 = '\n' as u8;
const MAX_BUF_SIZE: usize = 4 * 1024 * 1024; // 4 MiB

enum NEVER {}

fn version() {
    println!(
        "tac {} - Copyright NeoSmart Technologies 2017-2021",
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

#[allow(unused)]
/// This is the default, naïve byte search
fn search<W: Write>(bytes: &[u8], output: &mut W) -> Result<(), std::io::Error> {
    let mut last_printed = bytes.len() as i64;
    let mut index = last_printed - 1;

    while index > -2 {
        if index == -1 || bytes[index as usize] == SEARCH {
            output.write_all(&bytes[(index + 1) as usize..last_printed as usize])?;
            last_printed = index + 1;
        }

        index -= 1;
    }

    Ok(())
}

#[allow(unused)]
#[cfg(debug_assertions)]
/// Helper function to print the contents of a binary window as ASCII, for debugging purposes.
unsafe fn dump_window(window: *const u8) {
    let mut window_contents = [' '; 32];
    for i in 0..32 {
        window_contents[i] = window.add(i).read() as char;
    }
    dbg!(window_contents);
}

#[inline(always)]
/// Search a range index-by-index and write to `output` when a match is found.
fn slow_search_and_print(
    bytes: &[u8],
    start: usize,
    end: usize,
    stop: &mut usize,
    output: &mut dyn Write,
) -> Result<(), std::io::Error> {
    let mut i = end;
    while i > start {
        i -= 1;
        if bytes[i] == SEARCH {
            output.write_all(&bytes[i + 1..*stop])?;
            *stop = i + 1;
        }
    }

    Ok(())
}

#[target_feature(enable = "avx2")]
#[target_feature(enable = "lzcnt")]
#[target_feature(enable = "bmi2")]
#[allow(unused_unsafe)]
/// This isn't in the hot path, so prefer dynamic dispatch over a generic `Write` output.
/// This is an AVX2-optimized newline search function that searches a 32-byte (256-bit) window
/// instead of scanning character-by-character (once aligned). This is a *safe* function, but must
/// be adorned with `unsafe` to guarantee it's not called without first checking for AVX2 support.
///
/// We need to explicitly enable lzcnt support for u32::leading_zeros() to use the `lzcnt`
/// instruction instead of an extremely slow combination of branching + BSR. We do not need to test
/// for lzcnt support before calling this method as lzcnt was introduced by AMD alongside SSE4a, long
/// before AVX2, and by Intel on Haswell.
///
/// BMI2 is explicitly opted into to inline the BZHI instruction; otherwise a call to the intrinsic
/// function is added and not inlined.
unsafe fn search256<W: Write>(bytes: &[u8], mut output: &mut W) -> Result<(), std::io::Error> {
    let ptr = bytes.as_ptr();
    let mut last_printed = bytes.len();
    let mut index = last_printed - 1;

    // We should only use 32-byte (256-bit) aligned reads w/ AVX2 intrinsics.
    // Search unaligned bytes via slow method so subsequent haystack reads are always aligned.
    if index >= 32 {
        // Regardless of whether or not the base pointer is aligned to a 32-byte address, we are
        // reading from an arbitrary offset (determined by the length of the lines) and so we must
        // first calculate a safe place to begin using SIMD operations from.
        let align_offset = unsafe { ptr.offset(index as isize).align_offset(32) };
        let aligned_index = index as usize + align_offset - 32;
        debug_assert!(
            aligned_index <= index as usize && aligned_index < last_printed && aligned_index > 0
        );
        debug_assert!(
            (ptr as usize + aligned_index as usize) % 32 == 0,
            "Adjusted index is still not at 256-bit boundary!"
        );

        // eprintln!("Unoptimized search from {} to {}", aligned_index, last_printed);
        slow_search_and_print(
            bytes,
            aligned_index,
            last_printed,
            &mut last_printed,
            &mut output,
        )?;
        index = aligned_index;
        drop(aligned_index);

        let pattern256 = unsafe { core::arch::x86_64::_mm256_set1_epi8(SEARCH as i8) };
        while index >= 32 {
            let window_end_offset = index;
            index -= 32;
            unsafe {
                let window = ptr.add(index);

                // Prevent inadvertent access to the wrong variable below
                #[allow(unused)]
                let index: NEVER;

                let search256 = core::arch::x86_64::_mm256_load_si256(
                    window as *const core::arch::x86_64::__m256i,
                );
                let result256 = core::arch::x86_64::_mm256_cmpeq_epi8(search256, pattern256);
                let mut matches: u32 = core::arch::x86_64::_mm256_movemask_epi8(result256) as u32;

                while matches != 0 {
                    // We would count *trailing* zeroes to find new lines in reverse order, but the
                    // result mask is in little endian (reversed) order, so we do the very
                    // opposite.
                    // let leading = core::intrinsics::ctlz(matches)
                    let leading = matches.leading_zeros();
                    let offset = window_end_offset - leading as usize;

                    output.write_all(&bytes[offset..last_printed])?;
                    last_printed = offset;

                    // Clear this match from the matches bitset. The equivalent:
                    // matches &= !(1 << (32 - leading - 1));
                    matches = core::arch::x86_64::_bzhi_u32(matches, 31 - leading);
                }
            }
        }
    }

    if index != 0 {
        // eprintln!("Unoptimized end search from {} to {}", 0, index);
        slow_search_and_print(bytes, 0, index as usize, &mut last_printed, &mut output)?;
    }

    // Regardless of whether or not `index` is zero, as this is predicated on `last_printed`
    output.write_all(&bytes[0..last_printed])?;

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

        let mut output: &mut dyn Write = if force_flush || atty::is(atty::Stream::Stdout) {
            &mut output
        } else {
            buffered_output = BufWriter::new(output);
            &mut buffered_output
        };


        let mut use_avx2 = false;
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if is_x86_feature_detected!("avx2") {
                use_avx2 = true;
            }
        }
        if use_avx2 {
            unsafe { search256(bytes, &mut output)?; }
        } else {
            search(bytes, &mut output)?;
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

    Ok(())
}
