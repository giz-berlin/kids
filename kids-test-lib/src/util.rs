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
    pub const DEFAULT_MATRIX_HOMESERVER: &str = "localhost";
    pub const DEFAULT_AUTH_PROVIDER: &str = "keycloak";

    pub const DEFAULT_SOURCE_USER_ID: &str = "s0m3-us3r";
    pub const ANOTHER_SOURCE_USER_ID: &str = "4n0th3r-us3r";
    pub const DEFAULT_SOURCE_GROUP_ID: &str = "d3f4ult_gr0up";
    pub const ANOTHER_SOURCE_GROUP_ID: &str = "4n0th3r-gr0up";
    pub const THIRD_SOURCE_GROUP_ID: &str = "th1rd-gr0up";

    pub const DEFAULT_TARGET_USER_ID: &str = "@s0m3-us3r:localhost";
    pub const DEFAULT_TARGET_ROOM_ID: &str = "!default-target-room:localhost";
}
