use std::fs::File;
use std::io;

pub fn read_new_lines(
    file: &mut File,
    offset: &mut u64,
    partial: &mut String,
) -> io::Result<Vec<String>> {
    todo!()
}
