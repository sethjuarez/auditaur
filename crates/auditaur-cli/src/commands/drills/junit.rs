//! JUnit XML for CI dashboards, built from a finished run report.
//!
//! Follows Playwright's reporter: flaky tests pass, skipped and not-run tests are
//! `<skipped>`, and failures carry the first error. A suite error (app launch, hook)
//! marks its tests that never ran as `<error>`, or adds a
//! `(suite)` testcase when every test finished.

use std::fmt::Write;

use super::run::{Outcome, RunReport, Status, SuiteReport, TestReport, RUNS_DIR};

pub fn render(report: &RunReport) -> String {
    let counts: Vec<Counts> = report.suites.iter().map(Counts::of).collect();
    let sum = |field: fn(&Counts) -> usize| counts.iter().map(field).sum::<usize>();
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        xml,
        "<testsuites name=\"auditaur drills\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" errors=\"{}\" time=\"{}\">",
        sum(|c| c.tests),
        sum(|c| c.failures),
        sum(|c| c.skipped),
        sum(|c| c.errors),
        seconds(report.duration_ms),
    );
    for (suite, counts) in report.suites.iter().zip(&counts) {
        let _ = writeln!(
            xml,
            "  <testsuite name=\"{}\" file=\"{}\" timestamp=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" errors=\"{}\" time=\"{}\">",
            escape(&suite.suite),
            escape(&suite.path),
            escape(suite.started_at.as_deref().unwrap_or(&report.started_at)),
            counts.tests,
            counts.failures,
            counts.skipped,
            counts.errors,
            seconds(suite.duration_ms),
        );
        let suite_error = suite.error.as_ref().map(|error| &error.message);
        for test in &suite.tests {
            let body = testcase_body(report, suite_error, test);
            push_testcase(&mut xml, &test.name, suite, test.duration_ms, &body);
        }
        if counts.synthetic {
            // A failure no test can carry (e.g. a failing afterAll or teardown hook after
            // every test passed) still has to fail the CI report.
            let message = suite_error.map(String::as_str).unwrap_or_default();
            let body = format!(
                "      <error message=\"{}\" type=\"SuiteError\">{}</error>\n",
                escape(first_line(message)),
                escape(message),
            );
            push_testcase(&mut xml, "(suite)", suite, suite.duration_ms, &body);
        }
        xml.push_str("  </testsuite>\n");
    }
    xml.push_str("</testsuites>\n");
    xml
}

struct Counts {
    tests: usize,
    failures: usize,
    skipped: usize,
    errors: usize,
    /// The suite error gets its own testcase because no unrun test carries it.
    synthetic: bool,
}

impl Counts {
    fn of(suite: &SuiteReport) -> Self {
        let count = |pick: fn(Option<Outcome>) -> bool| {
            suite.tests.iter().filter(|test| pick(test.outcome)).count()
        };
        let unrun = count(|outcome| outcome.is_none());
        let (unrun_errors, synthetic) = match suite.error {
            Some(_) => (unrun, unrun == 0),
            None => (0, false),
        };
        Self {
            tests: suite.tests.len() + usize::from(synthetic),
            failures: count(|outcome| outcome == Some(Outcome::Unexpected)),
            skipped: count(|outcome| outcome == Some(Outcome::Skipped)) + unrun - unrun_errors,
            errors: unrun_errors + usize::from(synthetic),
            synthetic,
        }
    }
}

fn push_testcase(xml: &mut String, name: &str, suite: &SuiteReport, ms: Option<u64>, body: &str) {
    let _ = write!(
        xml,
        "    <testcase name=\"{}\" classname=\"{}\" file=\"{}\" time=\"{}\"",
        escape(name),
        escape(&suite.suite),
        escape(&suite.path),
        seconds(ms),
    );
    if body.is_empty() {
        xml.push_str("/>\n");
    } else {
        xml.push_str(">\n");
        xml.push_str(body);
        xml.push_str("    </testcase>\n");
    }
}
fn testcase_body(report: &RunReport, suite_error: Option<&String>, test: &TestReport) -> String {
    let mut body = String::new();
    let last = test.attempts.last();
    match test.outcome {
        Some(Outcome::Skipped) => {
            let reason = test.mode_reason.as_deref().unwrap_or("skipped");
            let _ = writeln!(body, "      <skipped message=\"{}\"/>", escape(reason));
        }
        None => match suite_error {
            Some(message) => {
                let _ = writeln!(
                    body,
                    "      <error message=\"{}\" type=\"SuiteError\"/>",
                    escape(first_line(message))
                );
            }
            None => body.push_str("      <skipped message=\"did not run\"/>\n"),
        },
        Some(Outcome::Unexpected) => {
            let (kind, message) = match (test.status, last.and_then(|a| a.error.as_ref())) {
                (Status::Passed, _) => (
                    "ExpectedFailure",
                    "passed but was expected to fail".to_string(),
                ),
                (status, Some(error)) => (failure_type(status), with_step(error)),
                (status, None) => (failure_type(status), status_word(status).to_string()),
            };
            let _ = writeln!(
                body,
                "      <failure message=\"{}\" type=\"{kind}\">{}</failure>",
                escape(first_line(&message)),
                escape(&message),
            );
        }
        Some(Outcome::Expected | Outcome::Flaky) => {}
    }
    if let Some(last) = last {
        let mut out = String::new();
        if test.outcome == Some(Outcome::Flaky) {
            let _ = writeln!(out, "flaky: passed on retry #{}", last.attempt);
        }
        if test.outcome == Some(Outcome::Unexpected) {
            let _ = writeln!(out, "attempt: {RUNS_DIR}/{}/{}", report.run_id, last.dir);
        }
        if !out.is_empty() {
            let _ = writeln!(body, "      <system-out>{}</system-out>", escape(&out));
        }
    }
    body
}

fn with_step(error: &super::run::ErrorInfo) -> String {
    match error.step {
        Some(step) => format!("step {}: {}", step + 1, error.message),
        None => error.message.clone(),
    }
}

fn failure_type(status: Status) -> &'static str {
    match status {
        Status::TimedOut => "TimedOut",
        Status::Interrupted => "Interrupted",
        _ => "AssertionError",
    }
}

fn status_word(status: Status) -> &'static str {
    match status {
        Status::TimedOut => "timed out",
        Status::Interrupted => "interrupted",
        _ => "failed",
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

fn seconds(ms: Option<u64>) -> String {
    format!("{:.3}", ms.unwrap_or(0) as f64 / 1000.0)
}

/// Escape text for XML attributes and content, dropping characters XML 1.0 forbids.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            ch if (ch as u32) < 0x20 || matches!(ch as u32, 0xFFFE | 0xFFFF) => {}
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_handles_markup_and_control_characters() {
        assert_eq!(
            escape("a<b & \"c\"\n\u{1b}[31m"),
            "a&lt;b &amp; &quot;c&quot;&#10;[31m"
        );
    }
}
