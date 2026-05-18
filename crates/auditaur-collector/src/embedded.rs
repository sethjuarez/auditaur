#[derive(Debug, Clone)]
pub struct EmbeddedCollector {
    pub session_id: String,
}

impl EmbeddedCollector {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
        }
    }
}
