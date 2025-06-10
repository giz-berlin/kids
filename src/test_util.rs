use std::fmt;

/// A number type whose default value is not static but *random* for each newly constructed instance.
#[derive(Debug, Clone, Copy)]
pub struct RandomId(u32);

impl Default for RandomId {
    fn default() -> Self {
        RandomId(rand::random())
    }
}

impl fmt::Display for RandomId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub mod constants {
    pub const DEFAULT_USER_ID: &str = "s0m3-us3r";
    pub const ANOTHER_USER_ID: &str = "4n0th3r-us3r";
    pub const DEFAULT_GROUP_ID: &str = "d3f4ult_gr0up";
    pub const ANOTHER_GROUP_ID: &str = "4n0th3r-gr0up";
}
