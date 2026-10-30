//! Native dialog stubs for `dialog` steps, served by `withDrillDialogs` in `@auditaur/api`.
//!
//! When a suite has a `dialog` step, the runner arms the app's stubs for every test attempt and
//! suite hook phase. Armed, the app answers dialogs only from the queue `dialog` steps fill, and
//! a dialog with nothing queued fails instead of blocking on a window no one will click. At the
//! end of the phase the runner checks that every queued answer was used and nothing went wrong,
//! then disarms. Arming is a lease the app drops on its own if the drill dies.

use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{AppDriver, StepCtx, StepResult};
use crate::commands::drill::expand_drill_env;
use crate::commands::drills::{
    model::{DialogStep, DrillSuite, Step, StepKind},
    run::ErrorInfo,
};

/// Bridge calls that manage the stubs, as opposed to steps, get this long.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// How often the end check re-reads the queue while the app may still be opening a dialog.
const POLL: Duration = Duration::from_millis(100);
/// Slack on top of the phase's budget, so the lease outlives the steps it covers.
const LEASE_MARGIN: Duration = Duration::from_secs(60);

pub fn suite_uses_dialogs(suite: &DrillSuite) -> bool {
    let phases = [
        &suite.before_all,
        &suite.before_each,
        &suite.after_each,
        &suite.after_all,
    ];
    phases
        .into_iter()
        .chain(suite.tests.iter().map(|test| &test.steps))
        .flatten()
        .any(|step| matches!(step.kind, StepKind::Dialog(_)))
}

/// Armed stubs for the phase being run.
pub struct DialogStubs {
    lease: Duration,
    /// What the app logged before a reload reset it.
    log: Vec<Value>,
    /// Problems found before the end check (answers a reload dropped, a failed re-arm).
    problems: Vec<String>,
}

impl DialogStubs {
    /// Arm the app's stubs, covering `budget` plus a margin.
    pub fn arm(driver: &mut dyn AppDriver, budget: Duration) -> Result<Self, String> {
        let lease = budget.saturating_add(LEASE_MARGIN);
        arm(driver, lease)?;
        Ok(Self {
            lease,
            log: Vec::new(),
            problems: Vec::new(),
        })
    }

    /// Queue a `dialog` step's answer. `${NAME}` in the answer expands from `env`.
    pub fn queue(
        &self,
        driver: &mut dyn AppDriver,
        env: &[(String, String)],
        step: &Step,
        dialog: &DialogStep,
        ctx: &StepCtx<'_>,
    ) -> StepResult {
        let fail = |message: String| StepResult::Failed(ErrorInfo::new(message));
        let respond = match expand(&dialog.respond, env) {
            Ok(respond) => respond,
            Err(name) => {
                return fail(format!(
                    "`respond` uses ${{{name}}}, which is not in the drill env (suite `env` or a setup hook's env file)"
                ))
            }
        };
        let mut entry = json!({
            "id": format!("step {}", ctx.step_index + 1),
            "kind": dialog.kind,
            "respond": respond,
            "leaseMs": millis(self.lease),
        });
        if let Some(options) = &dialog.expect_options {
            entry["expect"] = options.clone();
        }
        let timeout = ctx.remaining().min(CALL_TIMEOUT);
        if timeout.is_zero() {
            return StepResult::TimedOut(ErrorInfo::new("no time left to queue the dialog"));
        }
        match driver.bridge("dialogs.queue", Some(entry.to_string()), timeout) {
            Ok(_) => StepResult::Passed,
            Err(error) => fail(format!("{}: {error}", step.title)),
        }
    }

    /// Keep what the app logged before a `reload` resets its stubs.
    pub fn before_reload(&mut self, driver: &mut dyn AppDriver) {
        match driver.bridge("dialogs.state", None, CALL_TIMEOUT) {
            Ok(state) => {
                self.log.extend(entries(&state, "log"));
                for entry in entries(&state, "queue") {
                    self.problems.push(format!(
                        "{} was dropped by a reload",
                        describe_queued(&entry)
                    ));
                }
            }
            Err(error) => self.problems.push(format!(
                "could not read dialog stubs before reload: {error}"
            )),
        }
    }

    /// Re-arm after a reload, which starts the page (and its stubs) over.
    pub fn after_reload(&mut self, driver: &mut dyn AppDriver) {
        if let Err(error) = arm(driver, self.lease) {
            self.problems.push(format!(
                "could not re-arm dialog stubs after reload: {error}"
            ));
        }
    }

    /// Check the phase's dialogs and disarm. Waits up to `settle` for queued answers the app is
    /// about to use, unless the phase already failed (then unused answers are expected).
    /// Returns what the app logged and every problem found.
    pub fn finish(
        mut self,
        driver: &mut dyn AppDriver,
        settle: Duration,
        failed: bool,
    ) -> (Value, Vec<String>) {
        let give_up = Instant::now() + settle;
        let mut state = driver.bridge("dialogs.state", None, CALL_TIMEOUT);
        while let Ok(current) = &state {
            let waiting = armed(current) && !entries(current, "queue").is_empty();
            if failed || !waiting || Instant::now() >= give_up {
                break;
            }
            thread::sleep(POLL);
            state = driver.bridge("dialogs.state", None, CALL_TIMEOUT);
        }
        // Disarm whatever happened; the final state is the one to judge.
        let disarmed = driver.bridge("dialogs.disarm", None, CALL_TIMEOUT);
        let state = match (disarmed, state) {
            (Ok(last), _) => last,
            (Err(_), Ok(last)) => last,
            (Err(error), Err(_)) => {
                self.problems
                    .push(format!("could not read dialog stubs: {error}"));
                Value::Null
            }
        };
        let mut problems = std::mem::take(&mut self.problems);
        if !state.is_null() {
            self.log.extend(entries(&state, "log"));
            // `dialogs.disarm` reports the state as it was, so `armed` is still meaningful.
            if !armed(&state) {
                problems.push(
                    "dialog stubs were disarmed before the end: the page reloaded or navigated (use a `reload` step so the drill re-arms them), or the lease lapsed during a long wait"
                        .to_string(),
                );
            }
            let queue = entries(&state, "queue");
            if !failed && !queue.is_empty() {
                let unused: Vec<String> = queue.iter().map(describe_queued).collect();
                let hint = if state.get("wrapped").and_then(Value::as_bool) == Some(true) {
                    ""
                } else {
                    "; the app never loaded `withDrillDialogs`, so it is opening dialogs straight from `@tauri-apps/plugin-dialog`"
                };
                problems.push(format!(
                    "queued dialog answers were never used: {}{hint}",
                    unused.join(", ")
                ));
            }
        }
        for entry in &self.log {
            if let Some(problem) = describe_logged(entry) {
                problems.push(problem);
            }
        }
        let log = Value::Array(self.log);
        (json!({ "log": log, "final": state }), problems)
    }
}

fn arm(driver: &mut dyn AppDriver, lease: Duration) -> Result<(), String> {
    let value = json!({ "leaseMs": millis(lease) }).to_string();
    driver
        .bridge("dialogs.arm", Some(value), CALL_TIMEOUT)
        .map(drop)
        .map_err(|error| format!("could not arm dialog stubs: {error}"))
}

fn armed(state: &Value) -> bool {
    state.get("armed").and_then(Value::as_bool) == Some(true)
}

fn entries(state: &Value, key: &str) -> Vec<Value> {
    state
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn text<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or("?")
}

fn describe_queued(entry: &Value) -> String {
    format!("{} (`{}`)", text(entry, "id"), text(entry, "kind"))
}

/// A logged dialog that went wrong, in words.
fn describe_logged(entry: &Value) -> Option<String> {
    let kind = text(entry, "kind");
    match text(entry, "outcome") {
        "unexpected" => Some(format!(
            "the app opened a `{kind}` dialog with nothing queued; add a `dialog: {kind}` step before the action that opens it"
        )),
        "mismatch" => Some(format!(
            "the app opened a `{kind}` dialog that did not match {}: {}",
            entry
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("the queued answer"),
            text(entry, "problem")
        )),
        _ => None,
    }
}

/// Expand `${NAME}` in the answer's strings; an unknown name is an error, never a literal path.
fn expand(value: &Value, env: &[(String, String)]) -> Result<Value, String> {
    match value {
        Value::String(text) => {
            let (expanded, _) = expand_drill_env(text, env);
            match unexpanded(&expanded) {
                Some(name) => Err(name),
                None => Ok(Value::String(expanded)),
            }
        }
        Value::Array(items) => items
            .iter()
            .map(|item| expand(item, env))
            .collect::<Result<_, _>>()
            .map(Value::Array),
        other => Ok(other.clone()),
    }
}

fn unexpanded(text: &str) -> Option<String> {
    let start = text.find("${")? + 2;
    let end = text[start..].find('}')?;
    Some(text[start..start + end].to_string())
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_expand_from_the_drill_env_and_reject_unknown_names() {
        let env = vec![("FIXTURE".to_string(), "/tmp/f".to_string())];
        assert_eq!(
            expand(&json!(["${FIXTURE}/a.md", "b.md"]), &env).unwrap(),
            json!(["/tmp/f/a.md", "b.md"])
        );
        assert_eq!(expand(&json!(true), &env).unwrap(), json!(true));
        assert_eq!(
            expand(&json!("${RUN_DIR}/out.docx"), &env).unwrap_err(),
            "RUN_DIR"
        );
    }

    #[test]
    fn logged_problems_say_what_to_do() {
        let unexpected = describe_logged(&json!({ "kind": "save", "outcome": "unexpected" }));
        assert!(unexpected.unwrap().contains("add a `dialog: save` step"));
        let mismatch = describe_logged(&json!({
            "kind": "open", "outcome": "mismatch", "id": "step 2",
            "problem": "options.multiple: expected true, got undefined"
        }))
        .unwrap();
        assert!(mismatch.contains("step 2: options.multiple"), "{mismatch}");
        assert_eq!(
            describe_logged(&json!({ "kind": "ask", "outcome": "answered" })),
            None
        );
    }
}
