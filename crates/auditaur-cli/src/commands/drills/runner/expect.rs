//! DOM assertions: judge one bridge `inspect` reply against a matcher.
//!
//! The driver re-inspects until the verdict passes or the expect timeout runs
//! out, so this stays a pure function that is easy to test.

use serde::Deserialize;
use serde_json::Value;

use crate::commands::drills::model::{normalize_whitespace, Matcher};

/// The bridge's `inspect` reply.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Inspection {
    pub count: u64,
    /// Set when a locator without `nth` matched several elements.
    #[serde(default)]
    pub strict: Option<String>,
    #[serde(default)]
    pub element: Option<ElementState>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ElementState {
    pub visible: bool,
    pub text: String,
    #[serde(default)]
    pub value: Option<String>,
    /// `true`, `false`, `"mixed"`, or null when the element cannot be checked.
    #[serde(default)]
    pub checked: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Pass,
    /// Not yet; keep retrying. Both sides are human-readable.
    Retry {
        expected: String,
        actual: String,
    },
    /// Retrying cannot help (strict violation, wrong element type).
    Fatal(String),
}

pub fn judge(matcher: &Matcher, not: bool, inspection: &Inspection) -> Verdict {
    if !matches!(matcher, Matcher::ToHaveCount(_)) {
        if let Some(strict) = &inspection.strict {
            return Verdict::Fatal(strict.clone());
        }
    }
    let el = inspection.element.as_ref();
    let missing = || "<element not found>".to_string();
    // Like Playwright, property matchers need an element even under `not`;
    // only presence and visibility matchers treat absence as an answer.
    let absence_answers = matches!(
        matcher,
        Matcher::ToExist | Matcher::ToBeVisible | Matcher::ToBeHidden | Matcher::ToHaveCount(_)
    );
    if el.is_none() && !absence_answers {
        let expected = match matcher {
            Matcher::ToHaveText(text) | Matcher::ToHaveValue(text) => text.to_string(),
            Matcher::ToContainText(text) => format!("text containing {text:?}"),
            _ => "checked".to_string(),
        };
        return Verdict::Retry {
            expected: if not {
                format!("not {expected}")
            } else {
                expected
            },
            actual: missing(),
        };
    }
    let (passed, expected, actual) = match matcher {
        Matcher::ToExist => (
            el.is_some(),
            "attached".to_string(),
            if el.is_some() {
                "attached".to_string()
            } else {
                missing()
            },
        ),
        Matcher::ToBeVisible | Matcher::ToBeHidden => {
            let visible = el.is_some_and(|el| el.visible);
            let want_visible = matches!(matcher, Matcher::ToBeVisible);
            let actual = match el {
                None => missing(),
                Some(el) if el.visible => "visible".to_string(),
                Some(_) => "hidden".to_string(),
            };
            let expected = if want_visible { "visible" } else { "hidden" };
            (visible == want_visible, expected.to_string(), actual)
        }
        Matcher::ToHaveText(text) => match el {
            None => (false, text.to_string(), missing()),
            Some(el) => (text.matches(&el.text), text.to_string(), quote(&el.text)),
        },
        Matcher::ToContainText(text) => match el {
            None => (false, format!("text containing {text:?}"), missing()),
            Some(el) => (
                normalize_whitespace(&el.text).contains(&normalize_whitespace(text)),
                format!("text containing {text:?}"),
                quote(&el.text),
            ),
        },
        Matcher::ToHaveCount(count) => (
            inspection.count == *count,
            count.to_string(),
            inspection.count.to_string(),
        ),
        Matcher::ToHaveValue(value) => match el {
            None => (false, value.to_string(), missing()),
            Some(ElementState { value: None, .. }) => {
                return Verdict::Fatal(
                    "`toHaveValue` needs an input, textarea, or select".to_string(),
                )
            }
            Some(ElementState {
                value: Some(actual),
                ..
            }) => (value.matches(actual), value.to_string(), quote(actual)),
        },
        Matcher::ToBeChecked => match el {
            None => (false, "checked".to_string(), missing()),
            Some(el) => match &el.checked {
                Value::Bool(checked) => (
                    *checked,
                    "checked".to_string(),
                    if *checked { "checked" } else { "unchecked" }.to_string(),
                ),
                Value::String(mixed) if mixed == "mixed" => {
                    (false, "checked".to_string(), "mixed".to_string())
                }
                _ => {
                    return Verdict::Fatal(
                        "`toBeChecked` needs a checkbox, radio, or `aria-checked` element"
                            .to_string(),
                    )
                }
            },
        },
        Matcher::ToMatchAriaSnapshot(_) | Matcher::ToHaveScreenshot(_) => {
            return Verdict::Fatal(format!("`{}` is not a DOM assertion", matcher.key()))
        }
    };
    if passed != not {
        Verdict::Pass
    } else {
        Verdict::Retry {
            expected: if not {
                format!("not {expected}")
            } else {
                expected
            },
            actual,
        }
    }
}

fn quote(text: &str) -> String {
    format!("{:?}", normalize_whitespace(text))
}

#[cfg(test)]
mod tests {
    use regex::Regex;

    use super::*;
    use crate::commands::drills::model::TextMatch;

    fn el(text: &str) -> Inspection {
        Inspection {
            count: 1,
            strict: None,
            element: Some(ElementState {
                visible: true,
                text: text.to_string(),
                value: None,
                checked: Value::Null,
            }),
        }
    }

    fn none() -> Inspection {
        Inspection::default()
    }

    fn retry(expected: &str, actual: &str) -> Verdict {
        Verdict::Retry {
            expected: expected.to_string(),
            actual: actual.to_string(),
        }
    }

    #[test]
    fn existence_and_visibility() {
        assert_eq!(judge(&Matcher::ToExist, false, &el("a")), Verdict::Pass);
        assert_eq!(
            judge(&Matcher::ToExist, false, &none()),
            retry("attached", "<element not found>")
        );
        assert_eq!(judge(&Matcher::ToExist, true, &none()), Verdict::Pass);
        let mut hidden = el("a");
        hidden.element.as_mut().unwrap().visible = false;
        assert_eq!(
            judge(&Matcher::ToBeVisible, false, &hidden),
            retry("visible", "hidden")
        );
        assert_eq!(judge(&Matcher::ToBeHidden, false, &hidden), Verdict::Pass);
        assert_eq!(judge(&Matcher::ToBeHidden, false, &none()), Verdict::Pass);
        assert_eq!(
            judge(&Matcher::ToBeVisible, true, &el("a")),
            retry("not visible", "visible")
        );
    }

    #[test]
    fn text() {
        let exact = Matcher::ToHaveText(TextMatch::Exact("Hello world".into()));
        assert_eq!(judge(&exact, false, &el("  Hello\n world ")), Verdict::Pass);
        assert_eq!(
            judge(&exact, false, &el("Hello")),
            retry("\"Hello world\"", "\"Hello\"")
        );
        let pattern = Matcher::ToHaveText(TextMatch::Pattern(Regex::new("^Hel+o").unwrap()));
        assert_eq!(judge(&pattern, false, &el("Hello there")), Verdict::Pass);
        assert_eq!(
            judge(&pattern, false, &none()),
            retry("/^Hel+o/", "<element not found>")
        );
        let contains = Matcher::ToContainText("lo  wor".into());
        assert_eq!(judge(&contains, false, &el("Hello world")), Verdict::Pass);
        assert_eq!(judge(&contains, true, &el("nope")), Verdict::Pass);
    }

    #[test]
    fn missing_elements_fail_property_matchers_even_under_not() {
        let text = TextMatch::Exact("x".into());
        for matcher in [
            Matcher::ToHaveText(text.clone()),
            Matcher::ToContainText("x".into()),
            Matcher::ToHaveValue(text),
            Matcher::ToBeChecked,
        ] {
            for not in [false, true] {
                assert!(
                    matches!(judge(&matcher, not, &none()), Verdict::Retry { ref actual, .. } if actual == "<element not found>"),
                    "{} not={not}",
                    matcher.key()
                );
            }
        }
        assert_eq!(judge(&Matcher::ToBeVisible, true, &none()), Verdict::Pass);
        assert_eq!(
            judge(&Matcher::ToBeHidden, true, &none()),
            retry("not hidden", "<element not found>")
        );
        assert_eq!(
            judge(&Matcher::ToHaveCount(0), false, &none()),
            Verdict::Pass
        );
    }

    #[test]
    fn count_ignores_strictness() {
        let many = Inspection {
            count: 3,
            strict: Some("strict mode violation".into()),
            element: None,
        };
        assert_eq!(judge(&Matcher::ToHaveCount(3), false, &many), Verdict::Pass);
        assert_eq!(
            judge(&Matcher::ToHaveCount(2), false, &many),
            retry("2", "3")
        );
        assert_eq!(
            judge(&Matcher::ToBeVisible, false, &many),
            Verdict::Fatal("strict mode violation".into())
        );
    }

    #[test]
    fn value_and_checked() {
        let mut input = el("");
        input.element.as_mut().unwrap().value = Some("abc".into());
        let value = Matcher::ToHaveValue(TextMatch::Exact("abc".into()));
        assert_eq!(judge(&value, false, &input), Verdict::Pass);
        assert!(matches!(judge(&value, false, &el("x")), Verdict::Fatal(_)));

        let mut checkbox = el("");
        checkbox.element.as_mut().unwrap().checked = Value::Bool(false);
        assert_eq!(
            judge(&Matcher::ToBeChecked, false, &checkbox),
            retry("checked", "unchecked")
        );
        assert_eq!(judge(&Matcher::ToBeChecked, true, &checkbox), Verdict::Pass);
        checkbox.element.as_mut().unwrap().checked = Value::String("mixed".into());
        assert_eq!(
            judge(&Matcher::ToBeChecked, false, &checkbox),
            retry("checked", "mixed")
        );
        assert!(matches!(
            judge(&Matcher::ToBeChecked, false, &el("x")),
            Verdict::Fatal(_)
        ));
    }
}
