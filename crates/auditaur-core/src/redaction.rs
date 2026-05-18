use serde_json::Value;
use std::collections::HashSet;

pub const DEFAULT_REDACTION_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "authorization",
    "api_key",
    "apikey",
    "key",
    "cookie",
    "set-cookie",
    "session",
    "credential",
    "connection_string",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionResult {
    pub value: Value,
    pub redacted: bool,
}

pub fn redact_json(value: &Value, extra_keys: &[String]) -> RedactionResult {
    let keys = redaction_key_set(extra_keys);
    let mut redacted = false;
    let value = redact_value(value, &keys, &mut redacted);
    RedactionResult { value, redacted }
}

fn redaction_key_set(extra_keys: &[String]) -> HashSet<String> {
    DEFAULT_REDACTION_KEYS
        .iter()
        .map(|key| key.to_ascii_lowercase())
        .chain(extra_keys.iter().map(|key| key.to_ascii_lowercase()))
        .collect()
}

fn redact_value(value: &Value, keys: &HashSet<String>, redacted: &mut bool) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    if keys.contains(&key.to_ascii_lowercase()) {
                        *redacted = true;
                        (key.clone(), Value::String("[REDACTED]".to_string()))
                    } else {
                        (key.clone(), redact_value(value, keys, redacted))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_value(item, keys, redacted))
                .collect(),
        ),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::redact_json;
    use serde_json::json;

    #[test]
    fn redacts_matching_keys_recursively() {
        let input = json!({
            "name": "auditaur",
            "nested": {
                "token": "abc",
                "items": [{ "api_key": "def" }]
            }
        });

        let result = redact_json(&input, &[]);

        assert!(result.redacted);
        assert_eq!(result.value["name"], "auditaur");
        assert_eq!(result.value["nested"]["token"], "[REDACTED]");
        assert_eq!(result.value["nested"]["items"][0]["api_key"], "[REDACTED]");
    }
}
