#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionLimits {
    pub max_session_bytes: u64,
    pub max_log_rows: u64,
    pub max_span_rows: u64,
    pub max_error_rows: u64,
}

impl Default for RetentionLimits {
    fn default() -> Self {
        Self {
            max_session_bytes: 256 * 1024 * 1024,
            max_log_rows: 100_000,
            max_span_rows: 100_000,
            max_error_rows: 10_000,
        }
    }
}
