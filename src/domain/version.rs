//! Browser version sources and comparison rules.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSource {
    ExecutableResource,
    ExecutableFileVersion,
    BraveProductVersion,
    ApplicationIni,
    InstallReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVersion {
    pub version: String,
    pub source: VersionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionRelation {
    Unknown,
    Same,
    UpstreamNewer,
    UpstreamOlder,
    Different,
}

pub fn normalize_version(version: &str) -> String {
    let version = version.trim();
    version
        .strip_prefix(['v', 'V'])
        .unwrap_or(version)
        .trim_start()
        .to_owned()
}

pub fn compare_versions(current: &str, upstream: &str) -> VersionRelation {
    let current = normalize_version(current);
    let upstream = normalize_version(upstream);
    if current.is_empty() || upstream.is_empty() {
        return VersionRelation::Unknown;
    }
    if current.eq_ignore_ascii_case(&upstream) {
        return VersionRelation::Same;
    }

    let Some((mut current_numbers, current_suffix)) = version_parts(&current) else {
        return VersionRelation::Different;
    };
    let Some((mut upstream_numbers, upstream_suffix)) = version_parts(&upstream) else {
        return VersionRelation::Different;
    };
    while current_numbers.ends_with(&[0]) {
        current_numbers.pop();
    }
    while upstream_numbers.ends_with(&[0]) {
        upstream_numbers.pop();
    }
    let width = current_numbers.len().max(upstream_numbers.len());
    current_numbers.resize(width, 0);
    upstream_numbers.resize(width, 0);
    match current_numbers.cmp(&upstream_numbers) {
        std::cmp::Ordering::Less => VersionRelation::UpstreamNewer,
        std::cmp::Ordering::Greater => VersionRelation::UpstreamOlder,
        std::cmp::Ordering::Equal if current_suffix.eq_ignore_ascii_case(upstream_suffix) => {
            VersionRelation::Same
        }
        std::cmp::Ordering::Equal => VersionRelation::Different,
    }
}

fn version_parts(version: &str) -> Option<(Vec<u64>, &str)> {
    let split = version
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(version.len());
    let core = &version[..split];
    if core.is_empty() {
        return None;
    }
    let mut numbers = Vec::new();
    for part in core.split('.') {
        if part.is_empty() {
            return None;
        }
        numbers.push(part.parse().ok()?);
    }
    Some((numbers, &version[split..]))
}

#[cfg(test)]
mod tests {
    use super::{VersionRelation, compare_versions, normalize_version};

    #[test]
    fn normalization_removes_outer_whitespace_and_one_leading_v() {
        assert_eq!(normalize_version(" v158.0.1 "), "158.0.1");
        assert_eq!(normalize_version("V1.23.2b"), "1.23.2b");
    }

    #[test]
    fn numerically_equal_versions_with_different_lengths_are_same() {
        assert_eq!(
            compare_versions("156.0.8078.48", "156.0.8078.48.0"),
            VersionRelation::Same
        );
    }

    #[test]
    fn numeric_components_determine_update_direction() {
        assert_eq!(
            compare_versions("155.0.1", "156.0.0"),
            VersionRelation::UpstreamNewer
        );
        assert_eq!(
            compare_versions("158.0.1", "157.0.9"),
            VersionRelation::UpstreamOlder
        );
    }

    #[test]
    fn opaque_suffixes_are_not_guessed_as_an_update_direction() {
        assert_eq!(
            compare_versions("1.23.2a", "1.23.2b"),
            VersionRelation::Different
        );
        assert_eq!(
            compare_versions("115.44.0esr", "128.0.0esr"),
            VersionRelation::UpstreamNewer
        );
    }

    #[test]
    fn unparseable_or_empty_versions_are_unknown() {
        assert_eq!(compare_versions("", "1.0"), VersionRelation::Unknown);
        assert_eq!(
            compare_versions("development", "release"),
            VersionRelation::Different
        );
    }
}
