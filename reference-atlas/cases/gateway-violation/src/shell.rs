use alloc::string::String;
use crate::gateway::Gateway;
pub fn load(_gateway: &dyn Gateway, path: &str) -> String { std::fs::read_to_string(path).unwrap() }
