//! The ecosystem mismatch warning `cmf install --local` prints: a profile
//! compiled for `rust` against a Python project is almost certainly the
//! wrong profile (see `CMV.md`, "Ecosystems and sensors"). The decision is
//! pure — the profile's declared `[select] ecosystems` against what the
//! atlas's sensors detected at the project root — and `main.rs` only prints
//! what it returns. A profile that declares no ecosystems filters nothing,
//! so it never warns; an atlas that declares no sensors, or one that detects
//! nothing, says so instead of naming a workspace ecosystem.

use intent_atlas::profile::Profile;
use intent_atlas::sensors::{Detection, undetected};

/// The warning to print, without its `warning: ` prefix, or `None` when the
/// profile declares no ecosystems or every declared one was detected.
pub fn warning(profile: &Profile, detection: &Detection) -> Option<String> {
    let declared = &profile.select.ecosystems;
    if declared.is_empty() {
        return None;
    }
    let (detected, reason): (&[String], &str) = match detection {
        Detection::NoSensors => (&[], "the atlas declares no sensors"),
        Detection::Detected(detected) if detected.is_empty() => (&[], "nothing was detected"),
        Detection::Detected(detected) => (detected, ""),
    };
    if detection != &Detection::NoSensors && undetected(declared, detected).is_empty() {
        return None;
    }
    let shows = if reason.is_empty() {
        format!("this project shows {}", detected.join(", "))
    } else {
        reason.to_string()
    };
    Some(format!("profile {} targets {} but {shows}", profile.id, declared.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(ecosystems: &[&str]) -> Profile {
        let ecosystems: Vec<String> = ecosystems.iter().map(ToString::to_string).collect();
        toml::from_str(&format!(
            r#"
id = "rust-shipping"
name = "AGENTS"
version = "0.1.0"
description = "d"
surface = "agent"
budget_tokens = 100

[select]
ecosystems = {ecosystems:?}
keys = ["craftsperson/rust/a"]
"#
        ))
        .expect("profile parses")
    }

    fn detected(names: &[&str]) -> Detection {
        Detection::Detected(names.iter().map(ToString::to_string).collect())
    }

    #[test]
    fn warns_when_a_declared_ecosystem_is_not_among_the_detected() {
        assert_eq!(
            warning(&profile(&["rust"]), &detected(&["python"])).as_deref(),
            Some("profile rust-shipping targets rust but this project shows python")
        );
        assert_eq!(
            warning(&profile(&["python", "uv"]), &detected(&["python"])).as_deref(),
            Some("profile rust-shipping targets python, uv but this project shows python"),
            "the declared list is shown whole; uv is what was missed"
        );
    }

    #[test]
    fn names_the_reason_when_nothing_could_be_or_was_detected() {
        assert_eq!(
            warning(&profile(&["rust"]), &Detection::NoSensors).as_deref(),
            Some("profile rust-shipping targets rust but the atlas declares no sensors")
        );
        assert_eq!(
            warning(&profile(&["rust"]), &detected(&[])).as_deref(),
            Some("profile rust-shipping targets rust but nothing was detected")
        );
    }

    #[test]
    fn stays_quiet_when_the_profile_declares_nothing_or_everything_was_detected() {
        assert_eq!(warning(&profile(&[]), &detected(&["python"])), None);
        assert_eq!(warning(&profile(&[]), &Detection::NoSensors), None);
        assert_eq!(warning(&profile(&["rust"]), &detected(&["rust"])), None);
        assert_eq!(
            warning(&profile(&["python", "uv"]), &detected(&["python", "uv", "rust"])),
            None
        );
    }
}
