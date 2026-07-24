//! The I/O gateway traits that make command logic testable, and their real and fake
//! implementations.
//!
//! Every side effect cmx-core performs — reading/writing files, running `git`,
//! reading the clock, or calling an LLM — goes through one of these traits, never
//! directly. Production code uses the [`real`] implementations (wired up by
//! [`crate::production::ProductionContext`]); tests use the in-memory [`fakes`]
//! (available behind the `test-support` feature) so the same command logic runs
//! without touching the real filesystem, network, or clock.

pub mod clock;
pub mod fakes;
pub mod filesystem;
pub mod git;
pub mod llm;
pub mod real;

pub use clock::Clock;
pub use filesystem::{DirEntry, Filesystem};
pub use git::GitClient;
pub use llm::LlmClient;
#[cfg(feature = "llm")]
pub use real::MojenticLlmClient;
pub use real::{RealFilesystem, RealGitClient, SystemClock};
