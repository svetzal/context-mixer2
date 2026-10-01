use alloc::string::String;
use crate::gateway::Gateway;
pub fn load(gateway: &dyn Gateway, path: &str) -> String { gateway.read(path) }
