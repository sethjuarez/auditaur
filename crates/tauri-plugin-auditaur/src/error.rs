use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub struct AuditaurError {
    message: String,
}

impl AuditaurError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for AuditaurError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AuditaurError {}
