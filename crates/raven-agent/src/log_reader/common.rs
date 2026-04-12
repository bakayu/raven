/// trim `\n` and/or `\r` from lines
pub fn normalize_line(raw: &str) -> String {
    raw.trim_end_matches('\n')
        .trim_end_matches('\r')
        .to_string()
}
