//! Terminal paste and dropped-path parsing.
use super::*;
/// Parses terminal file drops, including quoted, escaped, and file URI paths.
pub fn parse_dropped_paths(text: &str) -> Vec<PathBuf> {
    split_paths(text)
        .into_iter()
        .filter_map(|value| normalize_path(value).filter(|path| !path.as_os_str().is_empty()))
        .collect()
}

fn split_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut path = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in text.trim().chars() {
        if escaped {
            path.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            } else {
                path.push(character);
            }
        } else if quote.is_none() && character.is_whitespace() {
            if !path.is_empty() {
                paths.push(std::mem::take(&mut path));
            }
        } else {
            path.push(character);
        }
    }
    if escaped {
        path.push('\\');
    }
    if !path.is_empty() {
        paths.push(path);
    }
    paths
}

fn normalize_path(value: String) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let value = value.strip_prefix("file://").map_or_else(
        || value.to_owned(),
        |uri| percent_decode_str(uri).decode_utf8_lossy().into_owned(),
    );
    if value == "~" {
        return std::env::var_os("HOME").map(PathBuf::from);
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return std::env::var_os("HOME").map(|home| PathBuf::from(home).join(rest));
    }
    Some(PathBuf::from(value))
}
