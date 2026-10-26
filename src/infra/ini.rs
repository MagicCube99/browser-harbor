//! Line-preserving INI updates for user-editable runtime files.

use std::collections::BTreeMap;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct IniUpdate<'a> {
    pub section: &'a str,
    pub key: &'a str,
    pub value: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IniEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

pub fn read_ini(path: &Path) -> Result<(String, IniEncoding), IniFileError> {
    let bytes = fs::read(path).map_err(|source| IniFileError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    decode_ini(&bytes).ok_or_else(|| IniFileError::Encoding(path.to_path_buf()))
}

pub fn write_ini(path: &Path, text: &str, encoding: IniEncoding) -> Result<(), IniFileError> {
    let bytes = encode_ini(text, encoding);
    fs::write(path, bytes).map_err(|source| IniFileError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum IniFileError {
    #[error("INI encoding is invalid at {0}")]
    Encoding(PathBuf),
    #[error("INI file operation failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub fn decode_ini(bytes: &[u8]) -> Option<(String, IniEncoding)> {
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let text = decode_utf16(bytes, true)?;
        return Some((text, IniEncoding::Utf16Le));
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let text = decode_utf16(bytes, false)?;
        return Some((text, IniEncoding::Utf16Be));
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8(bytes.to_vec())
            .ok()
            .map(|text| (text, IniEncoding::Utf8Bom));
    }
    String::from_utf8(bytes.to_vec())
        .ok()
        .map(|text| (text, IniEncoding::Utf8))
}

pub fn encode_ini(text: &str, encoding: IniEncoding) -> Vec<u8> {
    match encoding {
        IniEncoding::Utf8 => text.as_bytes().to_vec(),
        IniEncoding::Utf8Bom => {
            let mut bytes = vec![0xef, 0xbb, 0xbf];
            bytes.extend_from_slice(text.as_bytes());
            bytes
        }
        IniEncoding::Utf16Le => {
            let mut bytes = vec![0xff, 0xfe];
            for value in text.encode_utf16() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes
        }
        IniEncoding::Utf16Be => {
            let mut bytes = vec![0xfe, 0xff];
            for value in text.encode_utf16() {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            bytes
        }
    }
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let values = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect::<Vec<_>>();
    String::from_utf16(&values).ok()
}

pub fn update_ini(input: &str, updates: &[IniUpdate<'_>]) -> String {
    let newline = if input.contains("\r\n") { "\r\n" } else { "\n" };
    let mut pending = BTreeMap::<String, Vec<(&str, &str, bool)>>::new();
    for update in updates {
        pending
            .entry(update.section.to_ascii_lowercase())
            .or_default()
            .push((update.key, update.value, false));
    }

    let mut output = Vec::new();
    let mut current_section = String::new();
    for line in input.lines() {
        if let Some(section) = parse_section(line) {
            flush_missing(&mut output, &current_section, &mut pending);
            current_section = section.to_ascii_lowercase();
            output.push(line.to_owned());
            continue;
        }
        if let Some((key, value_start)) = parse_key(line)
            && let Some(values) = pending.get_mut(&current_section)
            && let Some((_, new_value, seen)) = values
                .iter_mut()
                .find(|(wanted, _, _)| wanted.eq_ignore_ascii_case(key))
        {
            *seen = true;
            let prefix = &line[..=value_start];
            let old_value = &line[value_start + 1..];
            let comment = old_value.find(" ;").map_or("", |index| &old_value[index..]);
            output.push(format!("{prefix}{new_value}{comment}"));
            continue;
        }
        output.push(line.to_owned());
    }
    flush_missing(&mut output, &current_section, &mut pending);
    for (section, values) in pending {
        let section_needs_append = values.iter().any(|(_, _, seen)| !seen);
        if !section_needs_append {
            continue;
        }
        if !output.is_empty() && output.last().is_some_and(|line| !line.is_empty()) {
            output.push(String::new());
        }
        output.push(format!("[{section}]"));
        for (key, value, _) in values {
            output.push(format!("{key}={value}"));
        }
    }
    let mut result = output.join(newline);
    result.push_str(newline);
    result
}

pub fn ini_section_entries(input: &str, section: &str) -> Vec<(String, String)> {
    let mut current = String::new();
    let mut entries = Vec::new();
    for line in input.lines() {
        if let Some(found) = parse_section(line) {
            current = found.to_ascii_lowercase();
            continue;
        }
        if current == section.to_ascii_lowercase()
            && let Some((key, value_start)) = parse_key(line)
        {
            entries.push((key.to_owned(), line[value_start + 1..].trim().to_owned()));
        }
    }
    entries
}

pub fn update_ini_map(input: &str, section: &str, entries: &[(String, String)]) -> String {
    let newline = if input.contains("\r\n") { "\r\n" } else { "\n" };
    let wanted = section.to_ascii_lowercase();
    let mut lines = input.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut start = None;
    let mut end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        if let Some(found) = parse_section(line) {
            if start.is_some() {
                end = index;
                break;
            }
            if found.eq_ignore_ascii_case(&wanted) {
                start = Some(index);
            }
        }
    }

    let mut replacement = Vec::new();
    match start {
        Some(index) => {
            replacement.push(lines[index].clone());
            for line in &lines[index + 1..end] {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with(';') {
                    replacement.push(line.clone());
                }
            }
        }
        None => {
            if !lines.is_empty() && !lines.last().is_some_and(String::is_empty) {
                lines.push(String::new());
            }
            replacement.push(format!("[{wanted}]"));
            start = Some(lines.len());
            end = lines.len();
        }
    }
    replacement.extend(
        entries
            .iter()
            .map(|(key, value)| format!("{}={}", key.trim(), value.trim())),
    );
    let Some(start) = start else {
        return input.to_owned();
    };
    lines.splice(start..end, replacement);
    let mut output = lines.join(newline);
    output.push_str(newline);
    output
}

fn flush_missing(
    output: &mut Vec<String>,
    section: &str,
    pending: &mut BTreeMap<String, Vec<(&str, &str, bool)>>,
) {
    if let Some(values) = pending.get_mut(section) {
        for (key, value, seen) in values {
            if !*seen {
                output.push(format!("{key}={value}"));
                *seen = true;
            }
        }
    }
}

fn parse_section(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.starts_with(';') {
        return None;
    }
    trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
}

fn parse_key(line: &str) -> Option<(&str, usize)> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('[') {
        return None;
    }
    let equals = line.find('=')?;
    Some((line[..equals].trim(), equals))
}

#[cfg(test)]
mod tests {
    use super::{
        IniEncoding, IniUpdate, decode_ini, encode_ini, ini_section_entries, update_ini,
        update_ini_map,
    };

    #[test]
    fn updates_known_values_and_preserves_comments_unknown_keys_and_casing() {
        let input = "; header\r\n[general]\r\ndata_dir=old\r\nunknown=keep\r\n; note\r\n[other]\r\nvalue=1\r\n";
        let output = update_ini(
            input,
            &[
                IniUpdate {
                    section: "general",
                    key: "data_dir",
                    value: r"D:\Portable\Data",
                },
                IniUpdate {
                    section: "general",
                    key: "cache_dir",
                    value: r"D:\Portable\Cache",
                },
            ],
        );

        assert!(output.contains("; header\r\n"));
        assert!(output.contains("; note\r\n"));
        assert!(output.contains("unknown=keep\r\n"));
        assert!(output.contains("data_dir=D:\\Portable\\Data\r\n"));
        assert!(output.contains("cache_dir=D:\\Portable\\Cache\r\n"));
    }

    #[test]
    fn appends_missing_known_section_and_key() {
        let output = update_ini(
            "[General]\nPortable=1\n",
            &[IniUpdate {
                section: "Env",
                key: "TmpDataPath",
                value: "none",
            }],
        );
        assert!(output.contains("[env]\nTmpDataPath=none\n"));
    }

    #[test]
    fn utf16_bom_round_trips_without_becoming_utf8() {
        let original = "[general]\r\ndata_dir=old\r\n";
        let bytes = encode_ini(original, IniEncoding::Utf16Le);
        let (decoded, encoding) = decode_ini(&bytes).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(encoding, IniEncoding::Utf16Le);
        assert_eq!(&encode_ini(&decoded, encoding)[..2], &[0xff, 0xfe]);
    }

    #[test]
    fn semicolons_inside_command_values_are_not_mistaken_for_comments() {
        let command = r#""D:\App\clean.bat";taskkill /f /im chrome.exe;"#;
        let input = format!("[general]\r\nlaunch_on_exit={command}\r\n");
        let output = update_ini(
            &input,
            &[IniUpdate {
                section: "general",
                key: "launch_on_exit",
                value: command,
            }],
        );
        assert_eq!(output, input);
    }

    #[test]
    fn keymapping_section_replaces_entries_and_keeps_comments() {
        let input = "[general]\nkeep=1\n[keymapping]\n; mapping note\nF2=Ctrl+PageUp\nCtrl+H=command:34014\n[other]\nkeep=2\n";
        let output = update_ini_map(
            input,
            "keymapping",
            &[("F3".to_owned(), "Ctrl+PageDown".to_owned())],
        );
        assert!(output.contains("; mapping note\nF3=Ctrl+PageDown\n"));
        assert!(!output.contains("F2="));
        assert!(output.contains("[other]\nkeep=2\n"));
        assert_eq!(
            ini_section_entries(&output, "keymapping"),
            vec![("F3".to_owned(), "Ctrl+PageDown".to_owned())]
        );
    }
}
