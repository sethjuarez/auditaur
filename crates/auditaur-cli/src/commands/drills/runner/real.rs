//! Drives real Tauri apps through the Auditaur drive bridge.

use std::{
    collections::HashSet,
    path::PathBuf,
    process::Child,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;

use super::{AppDriver, HookResult, Host, StepCtx, StepResult};
use crate::{
    commands::{
        debug::{self, DebugStageStatus},
        drill,
        drills::{
            model::{DrillSuite, Locator, Step, StepKind, WaitState},
            run::ErrorInfo,
        },
        drive::{BridgeReply, BridgeSession, DriveAppSelector},
        workflow,
    },
    discovery::DiscoveredApp,
};

const LAUNCH_POLL: Duration = Duration::from_millis(500);
const WAIT_POLL: Duration = Duration::from_millis(100);
/// Bounded so failure evidence never hangs a run.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
const RELOAD_POLL_TIMEOUT: Duration = Duration::from_secs(2);

/// How each suite gets its app.
#[derive(Debug, Clone)]
pub enum LaunchSource {
    /// Spawn this command (from `-- <command>`).
    Command(Vec<String>),
    /// Spawn the `start` command from this config, with fresh ports each launch.
    Config(PathBuf),
    /// Drive an already running app.
    Attach,
}

pub struct RealHost {
    pub app: String,
    pub source: LaunchSource,
    pub require_frontend: bool,
    pub ready_timeout: Duration,
    pub workspace_root: PathBuf,
}

impl Host for RealHost {
    fn launch(&mut self, _suite: &DrillSuite) -> Result<Box<dyn AppDriver>> {
        let (command, environment) = match &self.source {
            LaunchSource::Attach => return self.attach(),
            LaunchSource::Command(command) => (command.clone(), Vec::new()),
            LaunchSource::Config(path) => {
                let config = workflow::load_launch_config(path)?
                    .ok_or_else(|| anyhow!("`{}` no longer exists", path.display()))?;
                let command = config
                    .command
                    .ok_or_else(|| anyhow!("`{}` has no `start` command", path.display()))?;
                (command, config.environment)
            }
        };
        self.spawn(&command, &environment)
    }

    fn run_hook(&mut self, hook: &Value, teardown: bool) -> Result<HookResult> {
        let run = drill::run_hook_json(&self.workspace_root, hook, teardown)?;
        Ok(HookResult {
            passed: run.passed,
            timed_out: run.timed_out,
            report: run.report,
        })
    }
}

impl RealHost {
    fn attach(&self) -> Result<Box<dyn AppDriver>> {
        let selector = DriveAppSelector {
            app: Some(self.app.clone()),
            session_id: None,
            instance_id: None,
            pid: None,
            latest: false,
            active: true,
        };
        let session = BridgeSession::connect(&selector).map_err(|error| {
            anyhow!(
                "{error:#}\nStart the app first, or pass `-- <command>` (or a config `start` command) so the drill launches it."
            )
        })?;
        Ok(Box::new(BridgeDriver {
            session,
            child: None,
        }))
    }

    fn spawn(
        &self,
        command: &[String],
        environment: &[(String, String)],
    ) -> Result<Box<dyn AppDriver>> {
        let preexisting = drill::matching_sessions(&self.app)?;
        let preexisting_ids: HashSet<&str> = preexisting
            .iter()
            .map(|app| app.instance_id.as_str())
            .collect();
        let started_at = OffsetDateTime::now_utc();
        let mut child = drill::spawn_command(command, environment)?;
        let deadline = Instant::now() + self.ready_timeout;
        let ready = self.wait_until_ready(&mut child, started_at, &preexisting_ids, deadline);
        match ready.and_then(|app| BridgeSession::connect(&drill::drive_selector(&app))) {
            Ok(session) => Ok(Box::new(BridgeDriver {
                session,
                child: Some(child),
            })),
            Err(error) => {
                let _ = drill::cleanup_child_tree(&mut child);
                Err(error)
            }
        }
    }

    fn wait_until_ready(
        &self,
        child: &mut Child,
        started_at: OffsetDateTime,
        preexisting_ids: &HashSet<&str>,
        deadline: Instant,
    ) -> Result<DiscoveredApp> {
        let timeout = self.ready_timeout.as_secs();
        let app = loop {
            if let Some(app) = drill::find_spawned_session(&self.app, started_at, preexisting_ids)?
            {
                break app;
            }
            if let Some(status) = child.try_wait()? {
                return Err(anyhow!(
                    "app command exited ({status}) before a new `{}` Auditaur session appeared",
                    self.app
                ));
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "timed out after {timeout}s waiting for a new `{}` Auditaur session",
                    self.app
                ));
            }
            thread::sleep(LAUNCH_POLL);
        };
        let selector = debug::DebugSelector {
            db: Some(PathBuf::from(&app.database_path)),
            app: None,
            session_id: Some(app.session_id.clone()),
            instance_id: Some(app.instance_id.clone()),
            pid: Some(app.pid),
            latest: false,
            active: false,
            cdp_port: None,
            require_frontend: self.require_frontend,
            require_drive_bridge: true,
        };
        loop {
            let status = debug::snapshot(&selector)?;
            if status.ready {
                return Ok(app);
            }
            if let Some(exit) = child.try_wait()? {
                return Err(anyhow!("app exited ({exit}) before it was ready"));
            }
            if Instant::now() >= deadline {
                let waiting = status
                    .stages
                    .iter()
                    .filter(|stage| {
                        !matches!(
                            stage.status,
                            DebugStageStatus::Ok | DebugStageStatus::Skipped
                        )
                    })
                    .map(|stage| format!("{}: {}", stage.name, stage.message))
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(anyhow!(
                    "timed out after {timeout}s waiting for the app to be ready ({waiting})"
                ));
            }
            thread::sleep(LAUNCH_POLL);
        }
    }
}

pub struct BridgeDriver {
    session: BridgeSession,
    /// The spawned app process; `None` when attached to a running app.
    child: Option<Child>,
}

impl Drop for BridgeDriver {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl AppDriver for BridgeDriver {
    fn app_info(&self) -> Value {
        let mut info = self.session.app_info();
        info["launched"] = json!(self.child.is_some());
        info
    }

    fn run_step(&mut self, step: &Step, ctx: &mut StepCtx<'_>) -> StepResult {
        match self.step(step, ctx) {
            Ok(()) => StepResult::Passed,
            Err(failure) => failure,
        }
    }

    fn capture_failure(&mut self, ctx: &mut StepCtx<'_>) {
        let index = ctx.step_index;
        let step_id = Some(format!("step-{index}"));
        if let Ok(png) =
            self.session
                .screenshot_png(None, ctx.test_id.clone(), step_id, CAPTURE_TIMEOUT)
        {
            let _ = ctx.writer.attach(
                "failure screenshot",
                "image/png",
                &format!("step-{index}-failure.png"),
                &png,
            );
        }
        if let Ok(reply) = self.request(ctx, "snapshot", None, None, CAPTURE_TIMEOUT) {
            if reply.ok {
                if let Ok(bytes) = serde_json::to_vec_pretty(&reply.payload) {
                    let _ = ctx.writer.attach(
                        "failure dom",
                        "application/json",
                        &format!("step-{index}-failure-dom.json"),
                        &bytes,
                    );
                }
            }
        }
    }

    fn exited(&mut self) -> Option<String> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => Some(format!("app process exited ({status})")),
            _ => None,
        }
    }

    fn close(&mut self) -> Result<()> {
        // Keep the handle until cleanup succeeds so `Drop` can retry.
        if let Some(child) = self.child.as_mut() {
            drill::cleanup_child_tree(child)?;
            self.child = None;
        }
        Ok(())
    }
}

type StepOutcome = std::result::Result<(), StepResult>;

fn failed(message: impl Into<String>) -> StepResult {
    StepResult::Failed(ErrorInfo::new(message))
}

fn timed_out(message: impl Into<String>) -> StepResult {
    StepResult::TimedOut(ErrorInfo::new(message))
}

fn unsupported(what: &str) -> StepResult {
    failed(format!("{what} is not supported by the drill runner yet"))
}

/// Bridge selectors carry the whole locator as JSON; the bridge resolves it strictly.
pub(crate) const LOCATOR_PREFIX: &str = "locator:";
/// The bridge's error when an action's target is missing (or not visible).
const SELECTOR_NOT_FOUND: &str = "selector not found";

pub(crate) fn bridge_selector(locator: &Locator) -> String {
    fn put<T: serde::Serialize>(spec: &mut Value, key: &str, value: Option<T>) {
        if let Some(value) = value {
            spec[key] = json!(value);
        }
    }
    let (kind, value, name, exact, nth) = match locator {
        Locator::Css(css) => ("css", css, None, false, None),
        Locator::CssNth { css, nth } => ("css", css, None, false, Some(*nth)),
        Locator::Role {
            role,
            name,
            exact,
            nth,
        } => ("role", role, name.as_ref(), *exact, *nth),
        Locator::Text { text, exact, nth } => ("text", text, None, *exact, *nth),
        Locator::TestId { id, nth } => ("testId", id, None, false, *nth),
        Locator::Label { label, exact, nth } => ("label", label, None, *exact, *nth),
        Locator::Placeholder {
            placeholder,
            exact,
            nth,
        } => ("placeholder", placeholder, None, *exact, *nth),
    };
    let mut spec = json!({ "kind": kind, "value": value });
    put(&mut spec, "name", name);
    put(&mut spec, "exact", exact.then_some(true));
    put(&mut spec, "nth", nth);
    format!("{LOCATOR_PREFIX}{spec}")
}

/// Name the target in an error unless the bridge already did.
fn about(target: &Locator, error: String) -> String {
    let target = target.to_string();
    if error.contains(&target) {
        error
    } else {
        format!("`{target}`: {error}")
    }
}

/// Old bridges hand `locator:` selectors to `querySelectorAll`, which rejects them.
fn explain(error: String) -> String {
    if error.contains("is not a valid selector") && error.contains(LOCATOR_PREFIX) {
        format!(
            "{error}\nThis app's @auditaur/api predates drill locators; upgrade it to drive this app with `auditaur drill test`."
        )
    } else {
        error
    }
}

impl BridgeDriver {
    fn step(&mut self, step: &Step, ctx: &mut StepCtx<'_>) -> StepOutcome {
        match &step.kind {
            StepKind::Click(target) => self.act(ctx, "click", target, None, Vec::new()),
            StepKind::Dblclick(target) => self.act(ctx, "dblclick", target, None, Vec::new()),
            StepKind::Hover(target) => self.act(ctx, "hover", target, None, Vec::new()),
            StepKind::Check(target) => self.act(ctx, "check", target, None, Vec::new()),
            StepKind::Uncheck(target) => self.act(ctx, "uncheck", target, None, Vec::new()),
            StepKind::Fill { target, value } => {
                self.act(ctx, "fill", target, Some(value.clone()), Vec::new())
            }
            StepKind::Type { target, value } => {
                self.act(ctx, "type", target, Some(value.clone()), Vec::new())
            }
            StepKind::Select { target, values } => self.act(
                ctx,
                "select",
                target,
                values.first().cloned(),
                values.clone(),
            ),
            StepKind::Press {
                key,
                target: Some(target),
            } => self.act(ctx, "press", target, Some(key.clone()), Vec::new()),
            StepKind::Press { key, target: None } => self
                .call(ctx, "press", None, Some(key.clone()), Vec::new())
                .map(drop),
            StepKind::WaitFor { target, state } => self.wait_for(ctx, target, *state),
            StepKind::Evaluate(expression) => self
                .call(ctx, "evaluate", None, Some(expression.clone()), Vec::new())
                .map(drop),
            StepKind::Reload => self.reload(ctx),
            StepKind::Sleep(ms) => {
                let wait = Duration::from_millis(*ms);
                if wait > ctx.remaining() {
                    thread::sleep(ctx.remaining());
                    return Err(timed_out(format!(
                        "sleep of {ms}ms exceeds the remaining time"
                    )));
                }
                thread::sleep(wait);
                Ok(())
            }
            StepKind::Screenshot { name, target } => self.screenshot(ctx, name, target.as_ref()),
            StepKind::Gate(_) => Err(unsupported("`gate`")),
            StepKind::Expect(expectation) => Err(unsupported(&format!(
                "`expect.{}`",
                expectation.matcher.key()
            ))),
            StepKind::Telemetry(_) => Err(unsupported("`telemetry`")),
        }
    }

    /// Send one bridge request tagged with the current test and step.
    fn request(
        &self,
        ctx: &StepCtx<'_>,
        action: &str,
        selector: Option<String>,
        value: Option<String>,
        timeout: Duration,
    ) -> Result<BridgeReply> {
        self.request_full(ctx, action, selector, value, Vec::new(), false, timeout)
    }

    #[allow(clippy::too_many_arguments)]
    fn request_full(
        &self,
        ctx: &StepCtx<'_>,
        action: &str,
        selector: Option<String>,
        value: Option<String>,
        values: Vec<String>,
        visible_only: bool,
        timeout: Duration,
    ) -> Result<BridgeReply> {
        self.session.request(
            action,
            selector,
            value,
            values,
            visible_only,
            ctx.test_id.clone(),
            Some(format!("step-{}", ctx.step_index)),
            timeout,
        )
    }

    /// A request that must succeed before the step deadline.
    fn call(
        &self,
        ctx: &StepCtx<'_>,
        action: &str,
        selector: Option<String>,
        value: Option<String>,
        values: Vec<String>,
    ) -> std::result::Result<BridgeReply, StepResult> {
        if ctx.expired() {
            return Err(timed_out(format!("{action}: step timed out")));
        }
        let reply = self.request_full(ctx, action, selector, value, values, false, ctx.remaining());
        match reply {
            Ok(reply) if reply.ok => Ok(reply),
            Ok(reply) => Err(failed(explain(format!(
                "{action} failed: {}",
                reply
                    .error
                    .unwrap_or_else(|| "bridge returned ok=false".to_string())
            )))),
            Err(_) if ctx.expired() => Err(timed_out(format!("{action}: step timed out"))),
            Err(error) => Err(failed(explain(format!("{action} failed: {error:#}")))),
        }
    }

    /// Auto-wait for the target to be visible, then act on it (like Playwright actionability).
    /// The action itself also requires visibility; if the target vanished in between, wait again.
    fn act(
        &self,
        ctx: &StepCtx<'_>,
        action: &str,
        target: &Locator,
        value: Option<String>,
        values: Vec<String>,
    ) -> StepOutcome {
        loop {
            self.poll(ctx, target, true, true)
                .map_err(|error| match error {
                    StepResult::TimedOut(_) => timed_out(format!(
                        "{action}: timed out waiting for `{target}` to be visible"
                    )),
                    other => other,
                })?;
            if ctx.expired() {
                return Err(timed_out(format!("{action}: step timed out")));
            }
            let reply = self.request_full(
                ctx,
                action,
                Some(bridge_selector(target)),
                value.clone(),
                values.clone(),
                true,
                ctx.remaining(),
            );
            match reply {
                Ok(reply) if reply.ok => return Ok(()),
                Ok(reply) if reply.error.as_deref() == Some(SELECTOR_NOT_FOUND) => {
                    thread::sleep(WAIT_POLL.min(ctx.remaining()));
                }
                Ok(reply) => {
                    return Err(failed(explain(format!(
                        "{action} `{target}` failed: {}",
                        reply
                            .error
                            .unwrap_or_else(|| "bridge returned ok=false".to_string())
                    ))))
                }
                Err(_) if ctx.expired() => {
                    return Err(timed_out(format!("{action}: step timed out")))
                }
                Err(error) => {
                    return Err(failed(explain(format!(
                        "{action} `{target}` failed: {error:#}"
                    ))))
                }
            }
        }
    }

    fn wait_for(&self, ctx: &StepCtx<'_>, target: &Locator, state: WaitState) -> StepOutcome {
        let (visible_only, want) = match state {
            WaitState::Attached => (false, true),
            WaitState::Detached => (false, false),
            WaitState::Visible => (true, true),
            WaitState::Hidden => (true, false),
        };
        self.poll(ctx, target, visible_only, want)
            .map_err(|error| match error {
                StepResult::TimedOut(_) => timed_out(format!(
                    "timed out waiting for `{target}` to be {}",
                    state.as_str()
                )),
                other => other,
            })
    }

    /// Poll `exists` until it equals `want` or the step deadline passes.
    fn poll(
        &self,
        ctx: &StepCtx<'_>,
        target: &Locator,
        visible_only: bool,
        want: bool,
    ) -> StepOutcome {
        let selector = bridge_selector(target);
        loop {
            if ctx.expired() {
                return Err(timed_out("wait: step timed out"));
            }
            let reply = self.request_full(
                ctx,
                "exists",
                Some(selector.clone()),
                None,
                Vec::new(),
                visible_only,
                ctx.remaining(),
            );
            match reply {
                Ok(reply) => {
                    // A missing element comes back as `exists: false` plus an
                    // informational error; only a reply without `exists` is fatal.
                    let Some(exists) = reply.payload.get("exists").and_then(Value::as_bool) else {
                        let error = reply
                            .error
                            .unwrap_or_else(|| "bridge reply had no `exists` field".to_string());
                        return Err(failed(explain(about(target, error))));
                    };
                    if exists == want {
                        return Ok(());
                    }
                }
                Err(_) if ctx.expired() => return Err(timed_out("wait: step timed out")),
                Err(error) => return Err(failed(explain(about(target, format!("{error:#}"))))),
            }
            thread::sleep(WAIT_POLL.min(ctx.remaining()));
        }
    }

    fn reload(&self, ctx: &StepCtx<'_>) -> StepOutcome {
        self.call(
            ctx,
            "evaluate",
            None,
            Some("setTimeout(() => location.reload(), 0), true".to_string()),
            Vec::new(),
        )?;
        thread::sleep(Duration::from_millis(300).min(ctx.remaining()));
        // The bridge comes back with the new page; wait until it has fully loaded.
        loop {
            if ctx.expired() {
                return Err(timed_out("reload: step timed out"));
            }
            let reply = self.request(
                ctx,
                "evaluate",
                None,
                Some("document.readyState".to_string()),
                ctx.remaining().min(RELOAD_POLL_TIMEOUT),
            );
            if let Ok(reply) = reply {
                if reply.ok && reply.payload.get("value") == Some(&json!("complete")) {
                    return Ok(());
                }
            }
            thread::sleep(WAIT_POLL.min(ctx.remaining()));
        }
    }

    fn screenshot(
        &self,
        ctx: &mut StepCtx<'_>,
        name: &str,
        target: Option<&Locator>,
    ) -> StepOutcome {
        if let Some(target) = target {
            self.poll(ctx, target, true, true)
                .map_err(|error| match error {
                    StepResult::TimedOut(_) => timed_out(format!(
                        "screenshot: timed out waiting for `{target}` to be visible"
                    )),
                    other => other,
                })?;
        }
        let selector = target.map(bridge_selector);
        if ctx.expired() {
            return Err(timed_out("screenshot: step timed out"));
        }
        let png = self
            .session
            .screenshot_png(
                selector,
                ctx.test_id.clone(),
                Some(format!("step-{}", ctx.step_index)),
                ctx.remaining(),
            )
            .map_err(|error| failed(format!("{error:#}")))?;
        let file = format!("step-{}-{}.png", ctx.step_index, file_safe(name));
        ctx.writer
            .attach(name, "image/png", &file, &png)
            .map_err(|error| failed(format!("could not save screenshot: {error:#}")))?;
        Ok(())
    }
}

fn file_safe(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if safe.is_empty() {
        "screenshot".to_string()
    } else {
        safe
    }
}

#[cfg(test)]
mod selector_tests {
    use super::*;

    fn spec(locator: Locator) -> Value {
        let selector = bridge_selector(&locator);
        let json = selector.strip_prefix(LOCATOR_PREFIX).expect("prefix");
        serde_json::from_str(json).expect("json")
    }

    #[test]
    fn css_omits_defaults() {
        assert_eq!(
            spec(Locator::Css("#a".into())),
            json!({"kind": "css", "value": "#a"})
        );
        assert_eq!(
            spec(Locator::CssNth {
                css: "li".into(),
                nth: -1
            }),
            json!({"kind": "css", "value": "li", "nth": -1})
        );
    }

    #[test]
    fn role_carries_name_exact_nth() {
        let locator = Locator::Role {
            role: "button".into(),
            name: Some("Save \"it\"".into()),
            exact: true,
            nth: Some(2),
        };
        assert_eq!(
            spec(locator),
            json!({"kind": "role", "value": "button", "name": "Save \"it\"", "exact": true, "nth": 2})
        );
    }

    #[test]
    fn every_kind_maps() {
        let text = Locator::Text {
            text: "Hi".into(),
            exact: false,
            nth: None,
        };
        assert_eq!(spec(text), json!({"kind": "text", "value": "Hi"}));
        let test_id = Locator::TestId {
            id: "t".into(),
            nth: Some(0),
        };
        assert_eq!(
            spec(test_id),
            json!({"kind": "testId", "value": "t", "nth": 0})
        );
        let label = Locator::Label {
            label: "Email".into(),
            exact: true,
            nth: None,
        };
        assert_eq!(
            spec(label),
            json!({"kind": "label", "value": "Email", "exact": true})
        );
        let placeholder = Locator::Placeholder {
            placeholder: "Search".into(),
            exact: false,
            nth: None,
        };
        assert_eq!(
            spec(placeholder),
            json!({"kind": "placeholder", "value": "Search"})
        );
    }

    #[test]
    fn explains_old_bridges() {
        let error = "click failed: 'locator:{}' is not a valid selector".to_string();
        assert!(explain(error).contains("upgrade it"));
        assert_eq!(explain("selector not found".into()), "selector not found");
    }
}
