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
