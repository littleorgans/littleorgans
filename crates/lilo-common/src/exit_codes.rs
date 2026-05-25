/// Successful execution.
pub const SUCCESS: i32 = 0;

/// Unexpected internal failure.
pub const INTERNAL: i32 = 1;

/// Typed domain failure, such as a missing entity.
pub const DOMAIN: i32 = 2;

/// Invalid user input or malformed command arguments.
pub const INPUT_VALIDATION: i32 = 3;

/// The lilo daemon is unavailable.
pub const DAEMON_UNAVAILABLE: i32 = 4;

/// Authorization denied.
pub const AUTHZ_DENIED: i32 = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_match_phase_one_table() {
        assert_eq!(SUCCESS, 0);
        assert_eq!(INTERNAL, 1);
        assert_eq!(DOMAIN, 2);
        assert_eq!(INPUT_VALIDATION, 3);
        assert_eq!(DAEMON_UNAVAILABLE, 4);
        assert_eq!(AUTHZ_DENIED, 5);
    }
}
