//! The [`LlmClient`] gateway trait (feature `llm`) for LLM-powered diff analysis.

use crate::error::Result;
use std::future::Future;
use std::pin::Pin;

/// Abstraction over LLM text generation used by the `diff` command.
///
/// The return type uses a boxed `Future` so that the trait is object-safe
/// (usable as `dyn LlmClient`).  Real code uses `MojenticLlmClient`;
/// tests inject a fake with a canned response.
pub trait LlmClient: Send + Sync {
    /// Send `system_prompt`/`user_prompt` to the LLM and return its generated text.
    fn analyze(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>>;
}
