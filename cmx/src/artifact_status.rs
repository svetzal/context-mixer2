use crate::types::LockEntry;

/// Returns `true` if an installed artifact is considered outdated relative to
/// the source.  Pure function — no I/O.
///
/// The three rules:
/// - No lock entry → outdated (artifact is untracked)
/// - `source_checksum` differs from lock's `source_checksum` → outdated (content changed in source)
/// - Version newly present in source (source has a version, lock entry has none) → outdated
pub fn source_outdated(
    lock_entry: Option<&LockEntry>,
    source_checksum: &str,
    source_version: Option<&str>,
) -> bool {
    match lock_entry {
        Some(entry) => {
            // Checksum changed
            if entry.source_checksum != source_checksum {
                return true;
            }
            // Installed without a version but source now has one
            if entry.version.is_none() && source_version.is_some() {
                return true;
            }
            false
        }
        // No lock entry — untracked
        None => true,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::make_lock_entry_with_checksum;
    use crate::types::ArtifactKind;

    fn make_lock_entry(source_checksum: &str, version: Option<&str>) -> LockEntry {
        make_lock_entry_with_checksum(
            ArtifactKind::Agent,
            version,
            "guidelines",
            "agents/my-agent.md",
            source_checksum,
        )
    }

    #[test]
    fn source_outdated_matching_checksum_not_outdated() {
        let entry = make_lock_entry("sha256:abc", Some("1.0.0"));
        assert!(!source_outdated(Some(&entry), "sha256:abc", Some("1.0.0")));
    }

    #[test]
    fn source_outdated_changed_checksum_is_outdated() {
        let entry = make_lock_entry("sha256:abc", Some("1.0.0"));
        assert!(source_outdated(Some(&entry), "sha256:xyz", Some("1.0.0")));
    }

    #[test]
    fn source_outdated_no_lock_entry_is_outdated() {
        assert!(source_outdated(None, "sha256:abc", Some("1.0.0")));
    }

    #[test]
    fn source_outdated_version_appeared_in_source_is_outdated() {
        // Installed without a version; source now carries one
        let entry = make_lock_entry("sha256:abc", None);
        assert!(source_outdated(Some(&entry), "sha256:abc", Some("1.0.0")));
    }

    #[test]
    fn source_outdated_both_unversioned_same_checksum_not_outdated() {
        let entry = make_lock_entry("sha256:abc", None);
        assert!(!source_outdated(Some(&entry), "sha256:abc", None));
    }
}
