//! Python `parse_version`, `is_newer_version` and
//! `is_component_version_newer`.

/// Python `parse_version`: `v1.2.3.4-beta.5` becomes
/// `(1, 2, 3, 4, stage rank, 5)`. Anything else is all zeroes.
#[must_use]
pub fn parse_version(value: &str) -> [u64; 6] {
    parse(value.trim()).unwrap_or([0; 6])
}

fn parse(value: &str) -> Option<[u64; 6]> {
    let value = value.strip_prefix('v').unwrap_or(value);
    let (numbers, stage) = match value.split_once('-') {
        Some((numbers, stage)) => (numbers, Some(stage)),
        None => (value, None),
    };
    let parts: Vec<&str> = numbers.split('.').collect();
    if !(2..=4).contains(&parts.len()) {
        return None;
    }
    let mut result = [0_u64; 6];
    for (slot, part) in result.iter_mut().zip(&parts) {
        *slot = digits(part)?;
    }
    result[4] = 4;
    if let Some(stage) = stage {
        let letters = stage
            .find(|character: char| !character.is_ascii_alphabetic())
            .unwrap_or(stage.len());
        if letters == 0 {
            return None;
        }
        let (name, rest) = stage.split_at(letters);
        let rest = rest
            .strip_prefix('.')
            .or_else(|| rest.strip_prefix('-'))
            .unwrap_or(rest);
        result[4] = match name.to_ascii_lowercase().as_str() {
            "alpha" => 1,
            "beta" => 2,
            "rc" => 3,
            _ => 4,
        };
        if !rest.is_empty() {
            result[5] = digits(rest)?;
        } else if stage.len() > letters {
            // A lone separator after the stage name does not match Python.
            return None;
        }
    }
    Some(result)
}

fn digits(value: &str) -> Option<u64> {
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse().ok())
        .flatten()
}

/// Python `is_newer_version`.
#[must_use]
pub fn is_newer_version(remote: &str, current: &str) -> bool {
    parse_version(remote) > parse_version(current)
}

/// Python `is_component_version_newer`: compares up to four digit groups.
#[must_use]
pub fn is_component_version_newer(remote: &str, current: &str) -> bool {
    let mut remote = component_parts(remote);
    let mut current = component_parts(current);
    let length = remote.len().max(current.len());
    remote.resize(length, 0);
    current.resize(length, 0);
    remote > current
}

fn component_parts(value: &str) -> Vec<u64> {
    let parts: Vec<u64> = value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .take(4)
        .map(|part| part.parse().unwrap_or(u64::MAX))
        .collect();
    if parts.is_empty() { vec![0] } else { parts }
}

#[cfg(test)]
mod tests {
    use super::{is_component_version_newer, is_newer_version, parse_version};

    #[test]
    fn versions_parse_like_python() {
        assert_eq!(parse_version("1.0.21"), [1, 0, 21, 0, 4, 0]);
        assert_eq!(parse_version(" v2.0 "), [2, 0, 0, 0, 4, 0]);
        assert_eq!(parse_version("1.2.3.4-beta.5"), [1, 2, 3, 4, 2, 5]);
        assert_eq!(parse_version("2.0.0-rc2"), [2, 0, 0, 0, 3, 2]);
        assert_eq!(parse_version("2.0.0-alpha-1"), [2, 0, 0, 0, 1, 1]);
        assert_eq!(parse_version("2.0.0-dev.1"), [2, 0, 0, 0, 4, 1]);
        assert_eq!(parse_version("2.0.0-beta"), [2, 0, 0, 0, 2, 0]);
        assert_eq!(parse_version("nightly"), [0; 6]);
        assert_eq!(parse_version("1"), [0; 6]);
        assert_eq!(parse_version("1.2.3.4.5"), [0; 6]);
        assert_eq!(parse_version("1.2-"), [0; 6]);
    }

    #[test]
    fn newer_versions_follow_python_order() {
        assert!(is_newer_version("1.0.22", "1.0.21"));
        assert!(is_newer_version("2.0.0", "2.0.0-rc1"));
        assert!(is_newer_version("2.0.0-beta.2", "2.0.0-beta.1"));
        assert!(!is_newer_version("1.0.21", "2.0.0-dev.1"));
        assert!(!is_newer_version("1.0.21", "1.0.21"));
    }

    #[test]
    fn component_versions_compare_digit_groups() {
        assert!(is_component_version_newer("2026.09.01", "2026.08.19"));
        assert!(!is_component_version_newer("2026.08.19", "2026.08.19"));
        assert!(is_component_version_newer("2026.08.19.1", "2026.08.19"));
        assert!(is_component_version_newer("1", ""));
        assert!(!is_component_version_newer("2025.1.1", "2026.1.1"));
    }
}
