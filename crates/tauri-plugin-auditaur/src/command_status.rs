//! Span status for instrumented commands, recorded by the `auditaur_command` and
//! `instrument_ipc` macros when the command returns.
//!
//! The macros call `(&&OutcomeProbe(&result)).auditaur_outcome()` at the command's concrete
//! return type. Method resolution then picks the most specific impl: a `Result` whose error
//! serializes (every Tauri command error does), any other `Result`, or a plain value (always OK).

use serde::Serialize;

/// Longest error kept as the span's status message.
const MAX_MESSAGE_CHARS: usize = 512;

pub struct OutcomeProbe<'a, T>(pub &'a T);

impl<T> Clone for OutcomeProbe<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for OutcomeProbe<'_, T> {}

pub enum Outcome {
    Ok,
    Error(Option<String>),
}

pub trait SerializableErrorOutcome {
    fn auditaur_outcome(self) -> Outcome;
}

impl<T, E: Serialize> SerializableErrorOutcome for &&OutcomeProbe<'_, Result<T, E>> {
    fn auditaur_outcome(self) -> Outcome {
        match self.0 {
            Ok(_) => Outcome::Ok,
            Err(error) => Outcome::Error(error_message(error)),
        }
    }
}

pub trait ResultOutcome {
    fn auditaur_outcome(self) -> Outcome;
}

impl<T, E> ResultOutcome for &OutcomeProbe<'_, Result<T, E>> {
    fn auditaur_outcome(self) -> Outcome {
        match self.0 {
            Ok(_) => Outcome::Ok,
            Err(_) => Outcome::Error(None),
        }
    }
}

pub trait PlainOutcome {
    fn auditaur_outcome(self) -> Outcome;
}

impl<T> PlainOutcome for OutcomeProbe<'_, T> {
    fn auditaur_outcome(self) -> Outcome {
        Outcome::Ok
    }
}

/// Record `outcome` on the current span as `otel.status_code` / `otel.status_message`.
pub fn record(outcome: Outcome) {
    let span = ::tracing::Span::current();
    match outcome {
        Outcome::Ok => {
            span.record("otel.status_code", "OK");
        }
        Outcome::Error(message) => {
            span.record("otel.status_code", "ERROR");
            if let Some(message) = message {
                span.record("otel.status_message", message.as_str());
            }
        }
    }
}

fn error_message<E: Serialize>(error: &E) -> Option<String> {
    let message = match serde_json::to_value(error).ok()? {
        serde_json::Value::String(text) => text,
        serde_json::Value::Null => return None,
        value => value.to_string(),
    };
    Some(match message.char_indices().nth(MAX_MESSAGE_CHARS) {
        Some((end, _)) => format!("{}…", &message[..end]),
        None => message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    enum AppError {
        NotFound { id: u32 },
    }

    struct Opaque;

    fn outcome_of_serializable(result: &Result<u8, AppError>) -> Outcome {
        (&&OutcomeProbe(result)).auditaur_outcome()
    }

    fn outcome_of_opaque(result: &Result<u8, Opaque>) -> Outcome {
        (&&OutcomeProbe(result)).auditaur_outcome()
    }

    fn outcome_of_plain(value: &String) -> Outcome {
        (&&OutcomeProbe(value)).auditaur_outcome()
    }

    #[test]
    fn probes_pick_the_most_specific_outcome() {
        assert!(matches!(outcome_of_serializable(&Ok(1)), Outcome::Ok));
        assert!(matches!(
            outcome_of_serializable(&Err(AppError::NotFound { id: 7 })),
            Outcome::Error(Some(message)) if message == r#"{"NotFound":{"id":7}}"#
        ));
        assert!(matches!(
            outcome_of_opaque(&Err(Opaque)),
            Outcome::Error(None)
        ));
        assert!(matches!(outcome_of_opaque(&Ok(1)), Outcome::Ok));
        assert!(matches!(outcome_of_plain(&"hi".to_string()), Outcome::Ok));
    }

    #[test]
    fn string_errors_become_the_message_and_long_ones_are_clipped() {
        let short: Result<(), String> = Err("disk full".into());
        assert!(matches!(
            (&&OutcomeProbe(&short)).auditaur_outcome(),
            Outcome::Error(Some(message)) if message == "disk full"
        ));
        let long: Result<(), String> = Err("x".repeat(600));
        let Outcome::Error(Some(message)) = (&&OutcomeProbe(&long)).auditaur_outcome() else {
            panic!("expected an error message");
        };
        assert_eq!(message.chars().count(), MAX_MESSAGE_CHARS + 1);
    }
}
