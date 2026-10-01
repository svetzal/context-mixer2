use alloc::string::String;
pub trait Gateway { fn read(&self, path: &str) -> String; }
