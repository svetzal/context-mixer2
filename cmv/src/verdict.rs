//! The validator output contract and cmv's own result types: the JSON
//! [`Verdict`] a validator writes to stdout, the per-intent [`State`], the
//! [`IntentOutcome`] cmv reports, and the [`Summary`] whose exit code gates the
//! run (see `CMV.md`, "Validator invocation protocol" and "Verdict states and
//! exit codes"). Pure data and arithmetic; nothing here touches a gateway.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Exit code when every required, applicable validator passed.
pub const EXIT_HELD: u8 = 0;
/// Exit code when a required validator failed, or, under
/// [`Strictness::Strict`], any intent was unchecked.
pub const EXIT_NOT_HELD: u8 = 1;
/// Exit code for a missing or malformed manifest, an unresolvable atlas, or
/// bad usage. Raised by the binary, never computed by [`summarize`].
pub const EXIT_USAGE: u8 = 2;

/// Whether an unchecked intent fails the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strictness {
    /// Unchecked intents fail the run, like a required failure.
    Strict,
    /// Unchecked intents are reported but do not affect the exit code.
    Lenient,
}

impl Strictness {
    /// Convert from the raw `--strict` flag, exactly once, at the CLI boundary.
    pub fn from_flag(strict: bool) -> Self {
        if strict {
            Strictness::Strict
        } else {
            Strictness::Lenient
        }
    }
}

/// The one JSON document a validator writes to stdout.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Verdict {
    /// Whether the condition the intent governs arose in this workspace.
    pub applicable: bool,
    /// Whether the workspace follows the intent, when applicable.
    pub followed: bool,
    /// Validator-specific facts behind the verdict; an empty object when
    /// omitted.
    #[serde(default = "empty_object")]
    pub signals: Value,
    /// Human-readable observations, one per line of output.
    #[serde(default)]
    pub evidence: Vec<String>,
    /// Where in the workspace the observations were made.
    #[serde(default)]
    pub locations: Vec<Location>,
}

/// A workspace location a validator points at, rendered `path:line` like a
/// linter diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// Path relative to the workspace root.
    pub path: String,
    /// One-based line number, when the validator knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
}

impl Location {
    /// `path:line`, or just `path` without a line.
    pub fn render(&self) -> String {
        match self.line {
            Some(line) => format!("{}:{line}", self.path),
            None => self.path.clone(),
        }
    }
}

/// Parse a validator's stdout as a [`Verdict`].
pub fn parse(stdout: &str) -> Result<Verdict, serde_json::Error> {
    serde_json::from_str(stdout)
}

impl Verdict {
    /// The state this verdict reports.
    pub fn state(&self) -> State {
        match (self.applicable, self.followed) {
            (false, _) => State::NotApplicable,
            (true, true) => State::Pass,
            (true, false) => State::Fail,
        }
    }
}

/// Exactly one of these per compiled intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum State {
    /// Validator ran; applicable and followed.
    Pass,
    /// Validator ran; applicable and not followed.
    Fail,
    /// Validator ran; the condition never arose.
    NotApplicable,
    /// No verdict: no validator for the workspace's languages, or the
    /// validator could not start, crashed, timed out, or wrote no verdict.
    Unchecked {
        /// Why no verdict was reached.
        reason: String,
    },
    /// The manifest lists the intent as dropped; the guidance never reached
    /// the artifact, so nothing is held against the code.
    Unguided {
        /// The manifest's drop reason, e.g. `budget`.
        reason: String,
    },
}

impl State {
    /// Fixed-width label for the human listing.
    pub fn label(&self) -> &'static str {
        match self {
            State::Pass => "PASS",
            State::Fail => "FAIL",
            State::NotApplicable => "N/A",
            State::Unchecked { .. } => "UNCHECKED",
            State::Unguided { .. } => "UNGUIDED",
        }
    }

    /// The reason carried by an unchecked or unguided state.
    pub fn reason(&self) -> Option<&str> {
        match self {
            State::Unchecked { reason } | State::Unguided { reason } => Some(reason),
            State::Pass | State::Fail | State::NotApplicable => None,
        }
    }
}

/// What cmv reports for one compiled intent.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntentOutcome {
    /// The record's stable `id`; `None` for a dropped intent whose record is
    /// no longer in the atlas.
    pub id: Option<String>,
    /// The path-derived catalog key.
    pub key: String,
    /// Language(s) of the validators that produced this outcome, sorted and
    /// comma-separated; `None` when no validator ran.
    pub language: Option<String>,
    /// Whether the validators behind this outcome gate the run.
    pub required: bool,
    /// The verdict state, with its reason when unchecked or unguided.
    #[serde(flatten)]
    pub state: State,
    /// What was checked, as the guidance rendered it; `None` when no
    /// validator ran.
    pub description: Option<String>,
    /// Validator signals; an empty object when no validator ran.
    pub signals: Value,
    /// Validator observations.
    pub evidence: Vec<String>,
    /// Workspace locations the observations point at.
    pub locations: Vec<Location>,
    /// Whether the record's bytes differ from the checksum the manifest
    /// recorded at compile time.
    pub stale: bool,
}

/// Counts per state, the adherence rate, and the resulting exit code.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    /// Intents that passed.
    pub pass: usize,
    /// Intents that failed, required or not.
    pub fail: usize,
    /// Intents whose condition never arose.
    pub not_applicable: usize,
    /// Intents with no verdict.
    pub unchecked: usize,
    /// Intents the manifest dropped.
    pub unguided: usize,
    /// `pass / (pass + fail)` rounded to four decimals; `None` when nothing
    /// was applicable.
    pub adherence_rate: Option<f64>,
    /// [`EXIT_HELD`] or [`EXIT_NOT_HELD`].
    pub exit_code: u8,
}

/// Fold outcomes into a [`Summary`].
///
/// The exit code is [`EXIT_NOT_HELD`] when any *required* intent failed — an
/// optional validator's failure is reported but never gates — or, under
/// [`Strictness::Strict`], when any intent was unchecked. Unguided intents
/// never gate: the guidance never arrived.
pub fn summarize(outcomes: &[IntentOutcome], strictness: Strictness) -> Summary {
    let count = |matches: fn(&State) -> bool| {
        outcomes.iter().filter(|outcome| matches(&outcome.state)).count()
    };
    let pass = count(|state| matches!(state, State::Pass));
    let fail = count(|state| matches!(state, State::Fail));
    let unchecked = count(|state| matches!(state, State::Unchecked { .. }));
    let required_failure = outcomes
        .iter()
        .any(|outcome| outcome.required && matches!(outcome.state, State::Fail));
    let strict_failure = strictness == Strictness::Strict && unchecked > 0;
    Summary {
        pass,
        fail,
        not_applicable: count(|state| matches!(state, State::NotApplicable)),
        unchecked,
        unguided: count(|state| matches!(state, State::Unguided { .. })),
        adherence_rate: adherence_rate(pass, fail),
        exit_code: if required_failure || strict_failure {
            EXIT_NOT_HELD
        } else {
            EXIT_HELD
        },
    }
}

/// `pass / (pass + fail)` to four decimals, exactly as the benchmark computes
/// it: the denominator is what the code had occasion to exhibit.
fn adherence_rate(pass: usize, fail: usize) -> Option<f64> {
    let applicable = pass + fail;
    if applicable == 0 {
        return None;
    }
    // Going through `u32` keeps the float conversion lossless; a manifest with
    // more than four billion intents is not a realistic input.
    let pass = f64::from(u32::try_from(pass).ok()?);
    let applicable = f64::from(u32::try_from(applicable).ok()?);
    Some((pass / applicable * 10_000.0).round() / 10_000.0)
}

/// Build a `Value::Object` with no members.
pub fn empty_object() -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An outcome with only what [`summarize`] reads.
    pub(crate) fn outcome(key: &str, state: State, required: bool) -> IntentOutcome {
        IntentOutcome {
            id: Some(format!("kb.intent.{key}")),
            key: key.to_string(),
            language: Some("rust".to_string()),
            required,
            state,
            description: Some("Checked.".to_string()),
            signals: empty_object(),
            evidence: vec![],
            locations: vec![],
            stale: false,
        }
    }

    fn unchecked(reason: &str) -> State {
        State::Unchecked {
            reason: reason.to_string(),
        }
    }

    #[test]
    fn parses_a_full_verdict() {
        let verdict = parse(
            r#"{
                "applicable": true,
                "followed": false,
                "signals": { "effectful_modules": ["src/http.rs"] },
                "evidence": ["no gateway trait is declared"],
                "locations": [{ "path": "src/http.rs", "line": 14 }, { "path": "src/lib.rs" }]
            }"#,
        )
        .unwrap();
        assert_eq!(verdict.state(), State::Fail);
        assert_eq!(verdict.signals, json!({ "effectful_modules": ["src/http.rs"] }));
        assert_eq!(verdict.evidence, ["no gateway trait is declared"]);
        assert_eq!(verdict.locations[0].render(), "src/http.rs:14");
        assert_eq!(verdict.locations[1].render(), "src/lib.rs");
    }

    #[test]
    fn optional_fields_default_when_missing() {
        let verdict = parse(r#"{"applicable": true, "followed": true}"#).unwrap();
        assert_eq!(verdict.state(), State::Pass);
        assert_eq!(verdict.signals, json!({}));
        assert!(verdict.evidence.is_empty());
        assert!(verdict.locations.is_empty());
    }

    #[test]
    fn not_applicable_wins_over_followed() {
        let verdict = parse(r#"{"applicable": false, "followed": true}"#).unwrap();
        assert_eq!(verdict.state(), State::NotApplicable);
        let verdict = parse(r#"{"applicable": false, "followed": false}"#).unwrap();
        assert_eq!(verdict.state(), State::NotApplicable);
    }

    #[test]
    fn required_fields_cannot_be_omitted() {
        assert!(parse(r#"{"followed": true}"#).is_err());
        assert!(parse(r#"{"applicable": true}"#).is_err());
        assert!(parse("not json").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn state_serializes_as_a_tag_with_reason_when_present() {
        assert_eq!(serde_json::to_value(State::Pass).unwrap(), json!({ "state": "pass" }));
        assert_eq!(
            serde_json::to_value(State::NotApplicable).unwrap(),
            json!({ "state": "not_applicable" })
        );
        assert_eq!(
            serde_json::to_value(unchecked("timed out after 60s")).unwrap(),
            json!({ "state": "unchecked", "reason": "timed out after 60s" })
        );
    }

    #[test]
    fn outcome_flattens_state_into_its_own_fields() {
        let value = serde_json::to_value(outcome("a", unchecked("why"), true)).unwrap();
        assert_eq!(value["state"], "unchecked");
        assert_eq!(value["reason"], "why");
        assert_eq!(value["key"], "a");
        assert_eq!(value["required"], true);
    }

    #[test]
    fn exit_code_table() {
        let cases: Vec<(&str, Vec<IntentOutcome>, Strictness, u8)> = vec![
            ("no intents", vec![], Strictness::Lenient, EXIT_HELD),
            ("no intents, strict", vec![], Strictness::Strict, EXIT_HELD),
            (
                "all pass",
                vec![
                    outcome("a", State::Pass, true),
                    outcome("b", State::Pass, false),
                ],
                Strictness::Strict,
                EXIT_HELD,
            ),
            (
                "required fail",
                vec![outcome("a", State::Fail, true)],
                Strictness::Lenient,
                EXIT_NOT_HELD,
            ),
            (
                "optional fail",
                vec![outcome("a", State::Fail, false)],
                Strictness::Strict,
                EXIT_HELD,
            ),
            (
                "not applicable",
                vec![outcome("a", State::NotApplicable, true)],
                Strictness::Strict,
                EXIT_HELD,
            ),
            (
                "unchecked",
                vec![outcome("a", unchecked("no validator"), true)],
                Strictness::Lenient,
                EXIT_HELD,
            ),
            (
                "unchecked, strict",
                vec![outcome("a", unchecked("no validator"), false)],
                Strictness::Strict,
                EXIT_NOT_HELD,
            ),
            (
                "unguided",
                vec![outcome(
                    "a",
                    State::Unguided {
                        reason: "budget".into(),
                    },
                    true,
                )],
                Strictness::Strict,
                EXIT_HELD,
            ),
        ];
        for (name, outcomes, strictness, expected) in cases {
            assert_eq!(summarize(&outcomes, strictness).exit_code, expected, "{name}");
        }
    }

    #[test]
    fn counts_every_state_and_rounds_adherence_to_four_decimals() {
        let outcomes = vec![
            outcome("a", State::Pass, true),
            outcome("b", State::Pass, false),
            outcome("c", State::Fail, false),
            outcome("d", State::NotApplicable, true),
            outcome("e", unchecked("timed out after 60s"), true),
            outcome(
                "f",
                State::Unguided {
                    reason: "budget".into(),
                },
                true,
            ),
        ];
        let summary = summarize(&outcomes, Strictness::Lenient);
        assert_eq!(
            summary,
            Summary {
                pass: 2,
                fail: 1,
                not_applicable: 1,
                unchecked: 1,
                unguided: 1,
                adherence_rate: Some(0.6667),
                exit_code: EXIT_HELD,
            }
        );
    }

    #[test]
    fn adherence_is_absent_without_applicable_verdicts() {
        let outcomes = vec![
            outcome("a", State::NotApplicable, true),
            outcome("b", unchecked("no validator"), true),
        ];
        assert_eq!(summarize(&outcomes, Strictness::Lenient).adherence_rate, None);
        assert_eq!(summarize(&[], Strictness::Lenient).adherence_rate, None);
    }

    #[test]
    fn adherence_is_exact_at_the_ends() {
        assert_eq!(
            summarize(&[outcome("a", State::Pass, true)], Strictness::Lenient).adherence_rate,
            Some(1.0)
        );
        assert_eq!(
            summarize(&[outcome("a", State::Fail, false)], Strictness::Lenient).adherence_rate,
            Some(0.0)
        );
    }
}
