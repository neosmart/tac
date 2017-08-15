# tac

`tac` is a high-performance, cross-platform rewrite of the [GNU `tac` utility](https://www.gnu.org/software/coreutils/manual/html_node/tac-invocation.html#tac-invocation) of Coreutils.

This `tac` implementation uses memory-mapped files on all supported operating systems and is written in rust for maximum integrity and safety.

## Usage

```
Usage: tac [OPTIONS] [FILE1..]
Write each FILE to standard output, last line first.
Reads from STTDIN if no file is specified.

Options:
  -v --version: Print version and exit.
  -h --help   : Print this help text and exit
```

`tac` reads lines from any combination of `stdin` and/or zero or more files and writes the lines to the output in reverse order.

Since `tac` is implemented via memory-mapped files, there is no limit on line length and no danger of memory exhaustion.

## Installation

`tac` may be installed via cargo, the rust package manager:

```
cargo install tac
```

## License and Copyright

`tac` is written by Mahmoud Al-Qudsi <mqudsi@neosmart.net> of NeoSmart Technologies, and released under the terms of the MIT public license. Copyright NeoSmart Technologies 2017. All rights not assigned by the MIT license are reserved. 

