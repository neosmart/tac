# tac

`tac` is a high-performance, simd-accelerated, cross-platform rewrite of the [GNU `tac` utility](https://www.gnu.org/software/coreutils/manual/html_node/tac-invocation.html#tac-invocation) from Coreutils, released under a BSD-compatible (MIT) license. `tac` reads input from a file (or from `stdin`, but [see below](#implementation-notes)) and then prints it line-by-line backwards.

This `tac` implementation uses simd-acceleration for new line detection and utilizes memory-mapped files on all supported operating systems. It is additionally written in rust for maximum integrity and safety.

## Who needs a faster `tac` anyway?

Good question. Try grepping through a multi-gigabyte web access log file in reverse chronological order (`tac --line-buffered access.log | grep foo`) and then get back to me.

## Usage

```
Usage: tac [OPTIONS] [FILE1..]
Write each FILE to standard output, last line first.
Reads from stdin if FILE is - or not specified.

Options:
  -h --help        Print this help text and exit
  -v --version     Print version and exit.
  --line-buffered  Always flush output after each line.
```

`tac` reads lines from any combination of `stdin` and/or zero or more files and writes the lines to the output in reverse order.

### Example

```
$ echo -e "hello\nworld" | tac
world
hello
```

## Installation

`tac` may be installed via cargo, the rust package manager:

```
cargo install tac
```

Help is humbly requested in getting `tac` into the package managers for various platforms. It's really a time-consuming task, especially for someone that only interacts with the various packaging tools once in a blue moon as opposed to on a daily basis.

## Implementation Notes

This implementation of `tac` uses the AVX2 instruction set to provide SIMD acceleration for the detection of new lines. The usage of memory-mapped files additionally boosts performance by avoiding slowdowns caused by context switches when reading from the input if speculative execution mitigations are enabled. It is significantly (2.55x if mitigations disabled, more otherwise) faster than the version of `tac` that ships with GNU Coreutils, in addition to being more liberally licensed.

**To obtain maximum performance:**

* Try not to pipe input into `tac`. e.g. instead of running `cat /usr/share/dict/words | tac`, run `tac /usr/share/dict/words` directly. Because `tac` by definition must reach the end-of-file before it can emit its input with the lines reversed, if you use `tac`'s `stdin` interface (e.g. `cat foo | tac`), it must buffer all `stdin` input before it can begin to process the results. `tac` will try to buffer in memory, but once it exceeds a certain high-water mark (currently 4 MiB), it switches to disk-based buffering (because it can't know how large the input is or if it will end up exceeding the available free memory).
* Always try to place `tac` at the _start_ of a pipeline where possible. Even if you can guarantee that the input to `tac` will not exceed the in-memory buffering limit (see above), `tac` is almost certainly faster than any other command in your pipeline, and if you are going to reverse the output, you will benefit most if you reverse it from the start. For example, instead of running `grep foo /var/log/nginx/access.log | tac`, run `tac /var/log/nginx/access.log | grep foo`. This will (significantly) reduce the amount of work that `grep` (or any other downstream executable in the pipeline) has to do before finding a match.
* Use line-buffered output mode (`tac --line-buffered`) if tac is piping into another command rather than writing to the tty directly. This gives you "live" streaming of results and lets you terminate much sooner if you're only looking for the first _n_ matches. e.g. `tac --line-buffered access.log | grep foo` will print its first match much, much sooner than `tac access.log | grep foo` would.
* In the same vein, if you are chaining the output of _n_ utilities, make sure that all commands up to _n_ - 1 are all using line-buffered mode unless you don't care about latency and only care about throughput. For example, to print the first two matches for some grep pattern: `tac --line-buffered access.log | grep --line-buffered | head -n2`.

## License and Copyright

`tac` is written by Mahmoud Al-Qudsi <<mqudsi@neosmart.net>> of NeoSmart Technologies, and released under the terms of the MIT public license. Copyright NeoSmart Technologies 2021. All rights not assigned by the MIT license are reserved.

