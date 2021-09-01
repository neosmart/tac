use memmap::Mmap;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufWriter;

const SEARCH: u8 = b'\n';
const MAX_BUF_SIZE: usize = 4 * 1024 * 1024; // 4 MiB

pub fn reverse_file(path: &str, force_flush: bool) -> std::io::Result<()> {
    let mmap;
    let mut buf;
    let mut temp_path = None;

    {
        let bytes = match path {
            "-" => loop {
                // Depending on what the STDIN fd actually points to, it may still be possible to
                // mmap the input (e.g. in case of `tac - < foo.txt`). See issue #2.
                #[cfg(target_family = "unix")]
                {
                    use std::os::unix::io::{AsRawFd, FromRawFd};
                    let stdin = std::io::stdin();
                    let raw_fd = stdin.as_raw_fd();
                    // No memmap crate exposes a `map()` function that takes a `RawFd`, so we have
                    // to create a dummy file just to pass the specific fd we want to the OS.
                    // unsafe: We'll mem::forget() the file so the fd never has more than one owner
                    let file = unsafe { File::from_raw_fd(raw_fd) };
                    let test_mmap = unsafe { Mmap::map(&file) };
                    std::mem::forget(file);
                    if let Ok(map) = test_mmap {
                        // eprintln!("Successfully memmapped stdin");
                        mmap = map;
                        break &mmap[..];
                    } else {
                        // eprintln!("Unable to memmap stdin, falling back to buffering");
                    }
                }

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

                break match &file {
                    None => &buf[0..total_read],
                    Some(temp_file) => {
                        mmap = unsafe { Mmap::map(&temp_file)? };
                        &mmap[..]
                    }
                };
            },
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

        if bytes.len() == 0 {
            // Do nothing. This avoids an underflow in the search functions which expect there to
            // be at least one byte.
        } else {
            search_auto(bytes, &mut output)?;
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

#[allow(unreachable_code)]
fn search_auto<W: Write>(bytes: &[u8], mut output: &mut W) -> Result<(), std::io::Error> {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if is_x86_feature_detected!("avx2") {
        return unsafe { search256(bytes, &mut output) };
    }

    #[cfg(all(feature = "nightly", target_arch = "aarch64"))]
    return search128(bytes, &mut output);

    search(bytes, &mut output)
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

#[cfg(any(
    target_arch = "x86",
    target_arch = "x86_64",
    all(feature = "nightly", target_arch = "aarch64")
))]
#[inline(always)]
/// Search a range index-by-index and write to `output` when a match is found. Primarily used to
/// search before/after the aligned portion of a range.
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

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
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
    use core::arch::x86_64::*;

    let ptr = bytes.as_ptr();
    let mut last_printed = bytes.len();
    let mut index = last_printed - 1;

    // We should only use 32-byte (256-bit) aligned reads w/ AVX2 intrinsics.
    // Search unaligned bytes via slow method so subsequent haystack reads are always aligned.
    if index >= 64 {
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

        let pattern256 = unsafe { _mm256_set1_epi8(SEARCH as i8) };
        while index >= 64 {
            let window_end_offset = index;
            unsafe {
                index -= 32;
                let search256 = _mm256_load_si256(ptr.add(index) as *const __m256i);
                let result256 = _mm256_cmpeq_epi8(search256, pattern256);
                let mut matches = _mm256_movemask_epi8(result256) as u64;

                // Partially unroll this loop by repeating the above again before handling results
                index -= 32;
                let search256 = _mm256_load_si256(ptr.add(index) as *const __m256i);
                let result256 = _mm256_cmpeq_epi8(search256, pattern256);
                matches = (matches << 32) | _mm256_movemask_epi8(result256) as u64;

                while matches != 0 {
                    // We would count *trailing* zeroes to find new lines in reverse order, but the
                    // result mask is in little endian (reversed) order, so we do the very
                    // opposite.
                    // core::intrinsics::ctlz() is not stabilized, but `u64::leading_zeros()` will
                    // use it directly if the lzcnt or bmi1 features are enabled.
                    let leading = matches.leading_zeros();
                    let offset = window_end_offset - leading as usize;

                    output.write_all(&bytes[offset..last_printed])?;
                    last_printed = offset;

                    // Clear this match from the matches bitset. The equivalent:
                    // matches &= !(1 << (64 - leading - 1));
                    matches = _bzhi_u64(matches, 63 - leading);
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

#[cfg(all(feature = "nightly", target_arch = "aarch64"))]
/// This is a NEON/AdvSIMD-optimized newline search function that searches a 16-byte (128-bit) window
/// instead of scanning character-by-character (once aligned).
fn search128<W: Write>(bytes: &[u8], mut output: &mut W) -> Result<(), std::io::Error> {
    use core::arch::aarch64::*;

    let ptr = bytes.as_ptr();
    let mut last_printed = bytes.len();
    let mut index = last_printed - 1;

    if index >= 64 {
        // ARMv8 loads do not have alignment *requirements*, but there can be performance penalties
        // (e.g. seems to be about 2% slowdown on Cortex-A72 with a 500MB file) so let's align.
        // Search unaligned bytes via slow method so subsequent haystack reads are always aligned.
        let align_offset = unsafe { ptr.offset(index as isize).align_offset(16) };
        let aligned_index = index as usize + align_offset - 16;

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

        let pattern128 = unsafe { vdupq_n_u8(SEARCH) };
        while index >= 64 {
            let window_end_offset = index;
            unsafe {
                index -= 16;
                let window = ptr.add(index);
                let search128 = vld1q_u8(window);
                let result128_0 = vceqq_u8(search128, pattern128);

                index -= 16;
                let window = ptr.add(index);
                let search128 = vld1q_u8(window);
                let result128_1 = vceqq_u8(search128, pattern128);

                index -= 16;
                let window = ptr.add(index);
                let search128 = vld1q_u8(window);
                let result128_2 = vceqq_u8(search128, pattern128);

                index -= 16;
                let window = ptr.add(index);
                let search128 = vld1q_u8(window);
                let result128_3 = vceqq_u8(search128, pattern128);

                // Bulk movemask as described in
                // https://branchfree.org/2019/04/01/fitting-my-head-through-the-arm-holes/
                let mut matches = {
                    let bit_mask: uint8x16_t = std::mem::transmute([
                        0x01u8, 0x02, 0x4, 0x8, 0x10, 0x20, 0x40, 0x80, 0x01, 0x02, 0x4, 0x8, 0x10,
                        0x20, 0x40, 0x80,
                    ]);
                    let t0 = vandq_u8(result128_3, bit_mask);
                    let t1 = vandq_u8(result128_2, bit_mask);
                    let t2 = vandq_u8(result128_1, bit_mask);
                    let t3 = vandq_u8(result128_0, bit_mask);
                    let sum0 = vpaddq_u8(t0, t1);
                    let sum1 = vpaddq_u8(t2, t3);
                    let sum0 = vpaddq_u8(sum0, sum1);
                    let sum0 = vpaddq_u8(sum0, sum0);
                    vgetq_lane_u64(vreinterpretq_u64_u8(sum0), 0)
                };

                while matches != 0 {
                    // We would count *trailing* zeroes to find new lines in reverse order, but the
                    // result mask is in little endian (reversed) order, so we do the very
                    // opposite.
                    let leading = matches.leading_zeros();
                    let offset = window_end_offset - leading as usize;

                    output.write_all(&bytes[offset..last_printed])?;
                    last_printed = offset;

                    // Clear this match from the matches bitset.
                    matches &= !(1 << (64 - leading - 1));
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
