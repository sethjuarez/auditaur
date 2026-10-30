//! Drill suites: unit-test style drill files under `.auditaur/drills`.

// The runner consumes the remaining model fields; drop this once it lands.
pub mod coverage;
#[allow(dead_code)]
mod junit;
pub mod locate;
pub mod model;
pub mod run;
pub mod runner;
pub mod surface;
mod test_command;

pub use test_command::{test, TestOptions};

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde_json::{json, Value};

use model::{DrillSuite, Issue, ParsedFile, TestCase, TestMode};

pub const DEFAULT_DRILLS_DIR: &str = ".auditaur/drills";
pub const DRILL_FILE_SUFFIX: &str = ".drill.json";
// Used by `drill init` and MCP tools in later steps.
#[allow(dead_code)]
pub const DRILL_SCHEMA: &str = include_str!("../../../assets/drill.schema.json");
#[allow(dead_code)]
pub const DRILL_SCHEMA_URL: &str = "https://auditaur.dev/schemas/drill.schema.json";
const SKIPPED_DIRS: &[&str] = &["__snapshots__", "node_modules", ".git"];

/// Find drill files under the given roots (files or directories), sorted and de-duplicated.
pub fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for root in roots {
        if root.is_file() {
            files.push(root.clone());
        } else if root.is_dir() {
            walk(root, &mut files)?;
        } else {
            return Err(anyhow!(
                "drill path `{}` does not exist",
                display_path(root)
            ));
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("failed to read `{}`", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                walk(&path, files)?;
            }
        } else if name.ends_with(DRILL_FILE_SUFFIX) {
            files.push(path);
        }
    }
    Ok(())
}

pub fn default_roots() -> Vec<PathBuf> {
    vec![PathBuf::from(DEFAULT_DRILLS_DIR)]
}

/// Parsed drill files: valid suites plus every file with issues.
#[derive(Debug, Default)]
pub struct Loaded {
    pub suites: Vec<DrillSuite>,
    pub invalid: Vec<ParsedFile>,
    pub files: usize,
}

/// Parse every file and apply cross-file checks such as unique suite names.
pub fn load(files: &[PathBuf]) -> Loaded {
    let mut loaded = Loaded {
        files: files.len(),
        ..Loaded::default()
    };
    let mut owners: HashMap<String, PathBuf> = HashMap::new();
    for file in files {
        let mut parsed = model::parse_file(file);
        if let Some(suite) = &parsed.suite {
            if let Some(owner) = owners.get(&suite.suite) {
                parsed.issues.push(Issue {
                    path: "suite".to_string(),
                    message: format!(
                        "suite name {:?} is already used by `{}`",
                        suite.suite,
                        display_path(owner)
                    ),
                });
            } else {
                owners.insert(suite.suite.clone(), file.clone());
            }
        }
        match parsed.suite.take() {
            Some(suite) if parsed.issues.is_empty() => loaded.suites.push(suite),
            _ => loaded.invalid.push(parsed),
        }
    }
    loaded
}

/// Playwright-style test selection.
#[derive(Debug, Default, Clone)]
pub struct Filters {
    /// Each matches a file path substring or an exact suite name; any may match.
    pub patterns: Vec<String>,
    pub grep: Option<Regex>,
    pub grep_invert: Option<Regex>,
    /// Any listed tag may match (OR).
    pub tags: Vec<String>,
}

impl Filters {
    pub fn new(
        patterns: Vec<String>,
        grep: Option<&str>,
        grep_invert: Option<&str>,
        tags: Vec<String>,
    ) -> Result<Self> {
        let compile = |pattern: Option<&str>, flag: &str| {
            pattern
                .map(|pattern| {
                    Regex::new(pattern).with_context(|| format!("invalid {flag} pattern"))
                })
                .transpose()
        };
        Ok(Self {
            patterns,
            grep: compile(grep, "--grep")?,
            grep_invert: compile(grep_invert, "--grep-invert")?,
            tags: tags
                .into_iter()
                .map(|tag| tag.trim_start_matches('@').to_string())
                .collect(),
        })
    }

    fn matches_suite(&self, suite: &DrillSuite) -> bool {
        if self.patterns.is_empty() {
            return true;
        }
        let path = display_path(&suite.path);
        self.patterns.iter().any(|pattern| {
            suite.suite == *pattern || path_contains(&path, &pattern.replace('\\', "/"))
        })
    }

    fn matches_test(&self, suite: &DrillSuite, test: &TestCase) -> bool {
        let tags = effective_tags(suite, test);
        let haystack = grep_haystack(suite, test, &tags);
        if let Some(grep) = &self.grep {
            if !grep.is_match(&haystack) {
                return false;
            }
        }
        if let Some(invert) = &self.grep_invert {
            if invert.is_match(&haystack) {
                return false;
            }
        }
        self.tags.is_empty() || self.tags.iter().any(|tag| tags.contains(tag))
    }
}

/// Path substring match; case-insensitive where the filesystem is.
fn path_contains(path: &str, pattern: &str) -> bool {
    if cfg!(windows) || cfg!(target_os = "macos") {
        path.to_lowercase().contains(&pattern.to_lowercase())
    } else {
        path.contains(pattern)
    }
}

pub fn title(suite: &DrillSuite, test: &TestCase) -> String {
    format!("{} > {}", suite.suite, test.name)
}

pub fn effective_tags(suite: &DrillSuite, test: &TestCase) -> Vec<String> {
    let mut tags = suite.tags.clone();
    for tag in &test.tags {
        if !tags.contains(tag) {
            tags.push(tag.clone());
        }
    }
    tags
}

fn grep_haystack(suite: &DrillSuite, test: &TestCase, tags: &[String]) -> String {
    let mut haystack = title(suite, test);
    for tag in tags {
        haystack.push_str(" @");
        haystack.push_str(tag);
    }
    haystack
}

/// A suite with the tests that survived filtering.
#[derive(Debug)]
pub struct Selected<'a> {
    pub suite: &'a DrillSuite,
    pub tests: Vec<&'a TestCase>,
}

/// Applies filters, then focus: if any selected test is `only`, keep only
/// focused tests across all suites (like Playwright's `test.only`).
pub fn select<'a>(suites: &'a [DrillSuite], filters: &Filters) -> Vec<Selected<'a>> {
    let selected: Vec<Selected<'a>> = suites
        .iter()
        .filter(|suite| filters.matches_suite(suite))
        .filter_map(|suite| {
            let tests: Vec<&TestCase> = suite
                .tests
                .iter()
                .filter(|test| filters.matches_test(suite, test))
                .collect();
            (!tests.is_empty()).then_some(Selected { suite, tests })
        })
        .collect();
    let focused = selected.iter().any(|selected| {
        selected
            .tests
            .iter()
            .any(|test| test.mode == TestMode::Only)
    });
    if !focused {
        return selected;
    }
    selected
        .into_iter()
        .filter_map(|mut selected| {
            selected.tests.retain(|test| test.mode == TestMode::Only);
            (!selected.tests.is_empty()).then_some(selected)
        })
        .collect()
}

/// Path relative to the current directory with forward slashes.
pub fn display_path(path: &Path) -> String {
    let relative = std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf))
        .unwrap_or_else(|| path.to_path_buf());
    relative.to_string_lossy().replace('\\', "/")
}

fn issues_json(file: &ParsedFile) -> Value {
    json!({
        "path": display_path(&file.path),
        "issues": file.issues,
    })
}

fn print_issues(file: &ParsedFile) {
    eprintln!("{}", display_path(&file.path));
    for issue in &file.issues {
        eprintln!("  {}", issue.to_string().replace('\n', "\n    "));
    }
}

pub struct ListOptions {
    pub filters: Filters,
    pub json: bool,
}

pub fn list(options: ListOptions) -> Result<i32> {
    let roots = default_roots();
    let files = discover_or_empty(&roots)?;
    let loaded = load(&files);
    let selected = select(&loaded.suites, &options.filters);
    let test_count: usize = selected.iter().map(|entry| entry.tests.len()).sum();
    let exit_code = i32::from(!loaded.invalid.is_empty());

    if options.json {
        let workspace = std::env::current_dir()?;
        let suites: Vec<Value> = selected
            .iter()
            .map(|entry| {
                let suite = entry.suite;
                let suite_path = run::workspace_path(&workspace, &suite.path);
                json!({
                    "suite": suite.suite,
                    "path": display_path(&suite.path),
                    "description": suite.description,
                    "tags": suite.tags,
                    "hooks": {
                        "setup": suite.setup.len(),
                        "teardown": suite.teardown.len(),
                        "beforeAll": suite.before_all.len(),
                        "beforeEach": suite.before_each.len(),
                        "afterEach": suite.after_each.len(),
                        "afterAll": suite.after_all.len(),
                    },
                    "tests": entry.tests.iter().map(|test| json!({
                        "id": run::test_id(&suite_path, &test.name),
                        "name": test.name,
                        "title": title(suite, test),
                        "description": test.description,
                        "tags": effective_tags(suite, test),
                        "mode": test.mode,
                        "modeReason": test.mode_reason,
                        "slow": test.slow,
                        "expectFail": test.expect_fail,
                        "steps": test.steps.iter().map(|step| json!({
                            "title": step.title,
                            "action": step.action,
                        })).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let output = json!({
            "root": roots.iter().map(|root| display_path(root)).collect::<Vec<_>>(),
            "suites": suites,
            "invalid": loaded.invalid.iter().map(issues_json).collect::<Vec<_>>(),
            "totals": { "suites": selected.len(), "tests": test_count, "files": loaded.files },
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(exit_code);
    }

    if loaded.files == 0 {
        println!("No drill files found under {DEFAULT_DRILLS_DIR}");
        return Ok(0);
    }
    for entry in &selected {
        let suite = entry.suite;
        let tags = format_tags(&suite.tags);
        println!("{}  ({}){tags}", suite.suite, display_path(&suite.path));
        for test in &entry.tests {
            let mode = match test.mode {
                TestMode::Run => String::new(),
                TestMode::Only => "  [only]".to_string(),
                mode => match &test.mode_reason {
                    Some(reason) => format!("  [{}: {reason}]", mode.as_str()),
                    None => format!("  [{}]", mode.as_str()),
                },
            };
            let steps = match test.steps.len() {
                1 => "1 step".to_string(),
                count => format!("{count} steps"),
            };
            println!(
                "  {}  ({steps}){mode}{}",
                test.name,
                format_tags(&test.tags)
            );
        }
    }
    println!(
        "{test_count} {} in {} {}",
        plural(test_count, "test", "tests"),
        selected.len(),
        plural(selected.len(), "suite", "suites")
    );
    if !loaded.invalid.is_empty() {
        eprintln!();
        eprintln!(
            "{} invalid drill {} (run `auditaur drill validate`):",
            loaded.invalid.len(),
            plural(loaded.invalid.len(), "file", "files")
        );
        for file in &loaded.invalid {
            print_issues(file);
        }
    }
    Ok(exit_code)
}

fn format_ms(ms: Option<u64>) -> String {
    match ms {
        None => "-".to_string(),
        Some(ms) if ms < 1000 => format!("{ms}ms"),
        Some(ms) => format!("{:.1}s", ms as f64 / 1000.0),
    }
}

fn format_totals(totals: &run::Totals) -> String {
    let mut parts = vec![format!("{} passed", totals.passed)];
    for (count, label) in [
        (totals.failed, "failed"),
        (totals.flaky, "flaky"),
        (totals.skipped, "skipped"),
        (totals.not_run, "not run"),
    ] {
        if count > 0 {
            parts.push(format!("{count} {label}"));
        }
    }
    parts.join(", ")
}

/// Readable runs and unreadable folders under `workspace`, newest first.
fn read_runs(workspace: &Path) -> Result<(Vec<run::RunReport>, Vec<Value>)> {
    let root = run::runs_root(&workspace);
    let mut runs = Vec::new();
    let mut unreadable = Vec::new();
    for id in run::run_ids(&root)?.into_iter().rev() {
        match run::read_run(&root.join(&id)) {
            Ok(report) => runs.push(report),
            Err(error) => unreadable.push(json!({ "runId": id, "error": format!("{error:#}") })),
        }
    }
    Ok((runs, unreadable))
}

/// The `drill runs --json` document for `workspace`.
pub fn runs_json(workspace: &Path) -> Result<Value> {
    let (runs, unreadable) = read_runs(workspace)?;
    Ok(json!({
        "root": run::RUNS_DIR,
        "runs": runs.iter().map(|report| json!({
            "runId": report.run_id,
            "status": report.status,
            "startedAt": report.started_at,
            "durationMs": report.duration_ms,
            "totals": report.totals,
        })).collect::<Vec<_>>(),
        "unreadable": unreadable,
    }))
}

/// The `drill show --json` document: the run report plus its workspace-relative `dir`.
pub fn show_json(workspace: &Path, selector: Option<&str>) -> Result<Value> {
    let dir = run::resolve_run(&run::runs_root(&workspace), selector)?;
    let report = run::read_run(&dir)?;
    let mut value = serde_json::to_value(&report)?;
    value["dir"] = json!(format!("{}/{}", run::RUNS_DIR, report.run_id));
    Ok(value)
}

/// One test of a run with every attempt's full report (steps, errors, attachments).
/// `test` is a test id, title (`suite > test`) or name; a name must be unique in the run.
pub fn test_detail_json(workspace: &Path, selector: Option<&str>, test: &str) -> Result<Value> {
    let dir = run::resolve_run(&run::runs_root(&workspace), selector)?;
    let report = run::read_run(&dir)?;
    let tests: Vec<&run::TestReport> = report
        .suites
        .iter()
        .flat_map(|suite| &suite.tests)
        .collect();
    let matched = match tests
        .iter()
        .find(|candidate| candidate.id == test || candidate.title == test)
    {
        Some(found) => *found,
        None => {
            let named: Vec<&&run::TestReport> = tests
                .iter()
                .filter(|candidate| candidate.name == test)
                .collect();
            match named.as_slice() {
                [found] => **found,
                [] => return Err(anyhow!("run {} has no test `{test}`", report.run_id)),
                many => {
                    return Err(anyhow!(
                        "`{test}` names {} tests in run {}; use the id or `suite > test` title",
                        many.len(),
                        report.run_id
                    ))
                }
            }
        }
    };
    let attempts: Vec<Value> = matched
        .attempts
        .iter()
        .map(|summary| match run::read_attempt(&dir, summary) {
            Ok(attempt) => {
                let mut value = serde_json::to_value(attempt).unwrap_or(Value::Null);
                value["dir"] = json!(summary.dir);
                value
            }
            Err(error) => {
                let mut value = serde_json::to_value(summary).unwrap_or(Value::Null);
                // A running attempt has no attempt.json yet; that is not a read failure.
                if summary.status != run::Status::Running {
                    value["readError"] = json!(format!("{error:#}"));
                }
                value
            }
        })
        .collect();
    let mut test_value = serde_json::to_value(matched)?;
    test_value["attempts"] = Value::Array(attempts);
    Ok(json!({
        "runId": report.run_id,
        "runStatus": report.status,
        "dir": format!("{}/{}", run::RUNS_DIR, report.run_id),
        "test": test_value,
    }))
}

/// `auditaur drill runs`: recorded runs, newest first.
pub fn runs(json: bool) -> Result<()> {
    let workspace = std::env::current_dir()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&runs_json(&workspace)?)?);
        return Ok(());
    }
    let (runs, unreadable) = read_runs(&workspace)?;
    if runs.is_empty() && unreadable.is_empty() {
        println!("No drill runs under {}", run::RUNS_DIR);
        return Ok(());
    }
    for report in &runs {
        println!(
            "{}  {:<11}  {}  ({})",
            report.run_id,
            report.status.as_str(),
            format_totals(&report.totals),
            format_ms(report.duration_ms)
        );
    }
    for entry in &unreadable {
        eprintln!("{}  unreadable: {}", entry["runId"], entry["error"]);
    }
    Ok(())
}

/// `auditaur drill show [run]`: one run's suites, tests and failures.
pub fn show(selector: Option<&str>, json: bool) -> Result<()> {
    let workspace = std::env::current_dir()?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&show_json(&workspace, selector)?)?
        );
        return Ok(());
    }
    let dir = run::resolve_run(&run::runs_root(&workspace), selector)?;
    let report = run::read_run(&dir)?;
    println!(
        "Run {}  {}  ({})",
        report.run_id,
        report.status.as_str(),
        format_ms(report.duration_ms)
    );
    for suite in &report.suites {
        println!();
        println!(
            "{}  ({})  {}",
            suite.suite,
            suite.path,
            suite.status.as_str()
        );
        if let Some(error) = &suite.error {
            println!("  error: {}", error.message.replace('\n', "\n    "));
        }
        for test in &suite.tests {
            let label = match test.outcome {
                Some(run::Outcome::Flaky) => "flaky",
                Some(run::Outcome::Skipped) => "skipped",
                Some(run::Outcome::Expected) if test.status == run::Status::Failed => "xfail",
                _ => test.status.as_str(),
            };
            let retries = match test.attempts.len() {
                0 | 1 => String::new(),
                count => format!("  [{count} attempts]"),
            };
            println!(
                "  {label:<11}  {}  ({}){retries}",
                test.name,
                format_ms(test.duration_ms)
            );
            if test.outcome == Some(run::Outcome::Unexpected) {
                if let Some(error) = test.attempts.last().and_then(|a| a.error.as_ref()) {
                    let at = error
                        .step
                        .map(|step| format!("step {}: ", step + 1))
                        .unwrap_or_default();
                    println!("      {at}{}", error.message.replace('\n', "\n      "));
                }
            }
        }
    }
    println!();
    println!("{}", format_totals(&report.totals));
    Ok(())
}

pub struct ValidateOptions {
    pub paths: Vec<PathBuf>,
    pub json: bool,
}

pub fn validate(options: ValidateOptions) -> Result<i32> {
    let explicit = !options.paths.is_empty();
    let roots = if explicit {
        options.paths
    } else {
        default_roots()
    };
    let files = if explicit {
        discover(&roots)?
    } else {
        discover_or_empty(&roots)?
    };
    let loaded = load(&files);
    let valid = !files.is_empty() && loaded.invalid.is_empty();

    if options.json {
        let output = json!({
            "valid": valid,
            "files": loaded.files,
            "suites": loaded.suites.iter().map(|suite| json!({
                "suite": suite.suite,
                "path": display_path(&suite.path),
                "tests": suite.tests.len(),
            })).collect::<Vec<_>>(),
            "invalid": loaded.invalid.iter().map(issues_json).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(i32::from(!valid));
    }

    if files.is_empty() {
        eprintln!(
            "No drill files found under {}",
            roots
                .iter()
                .map(|root| display_path(root))
                .collect::<Vec<_>>()
                .join(", ")
        );
        return Ok(1);
    }
    for suite in &loaded.suites {
        println!(
            "ok  {}  ({}, {} {})",
            display_path(&suite.path),
            suite.suite,
            suite.tests.len(),
            plural(suite.tests.len(), "test", "tests")
        );
    }
    for file in &loaded.invalid {
        print_issues(file);
    }
    let issue_count: usize = loaded.invalid.iter().map(|file| file.issues.len()).sum();
    if valid {
        println!(
            "{} drill {} valid",
            loaded.files,
            plural(loaded.files, "file", "files")
        );
    } else {
        eprintln!(
            "{issue_count} {} in {} {}",
            plural(issue_count, "issue", "issues"),
            loaded.invalid.len(),
            plural(loaded.invalid.len(), "file", "files")
        );
    }
    Ok(i32::from(!valid))
}

/// Missing default directories mean "no drills yet" rather than an error.
fn discover_or_empty(roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let existing: Vec<PathBuf> = roots.iter().filter(|root| root.exists()).cloned().collect();
    discover(&existing)
}

fn format_tags(tags: &[String]) -> String {
    if tags.is_empty() {
        String::new()
    } else {
        format!(
            "  {}",
            tags.iter()
                .map(|tag| format!("@{tag}"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 {
        one
    } else {
        many
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, relative: &str, text: &str) -> PathBuf {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    const SUITE: &str = r##"{
        "tags": ["smoke"],
        "tests": [
            { "name": "greets", "tags": ["fast"], "steps": [{ "click": "#a" }] },
            { "name": "errors", "steps": [{ "click": "#b" }] }
        ]
    }"##;

    #[test]
    fn discovers_sorted_drill_files_and_skips_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "b.drill.json", SUITE);
        write(dir.path(), "nested/a.drill.json", SUITE);
        write(dir.path(), "__snapshots__/x.drill.json", SUITE);
        write(dir.path(), "notes.json", "{}");
        let files = discover(&[dir.path().to_path_buf()]).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|file| {
                file.strip_prefix(dir.path())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(names, ["b.drill.json", "nested/a.drill.json"]);
        assert!(discover(&[dir.path().join("missing")]).is_err());
    }

    #[test]
    fn rejects_duplicate_suite_names_across_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "a/login.drill.json", SUITE);
        let b = write(dir.path(), "b/login.drill.json", SUITE);
        let loaded = load(&[a, b]);
        assert_eq!(loaded.suites.len(), 1);
        assert_eq!(loaded.invalid.len(), 1);
        assert!(loaded.invalid[0].issues[0]
            .message
            .contains("already used by"));
    }

    #[test]
    fn filters_by_pattern_grep_and_tag() {
        let dir = tempfile::tempdir().unwrap();
        let login = write(dir.path(), "login.drill.json", SUITE);
        let other = write(
            dir.path(),
            "other.drill.json",
            r##"{ "tests": [{ "name": "greets", "steps": [{ "click": "#a" }] }] }"##,
        );
        let loaded = load(&[login, other]);
        let names = |filters: Filters| -> Vec<String> {
            select(&loaded.suites, &filters)
                .iter()
                .flat_map(|entry| entry.tests.iter().map(|test| title(entry.suite, test)))
                .collect()
        };
        assert_eq!(names(Filters::default()).len(), 3);
        assert_eq!(
            names(Filters::new(vec!["login".into()], None, None, vec![]).unwrap()),
            ["login > greets", "login > errors"]
        );
        assert_eq!(
            names(Filters::new(vec![], Some("greets"), Some("@fast"), vec![]).unwrap()),
            ["other > greets"]
        );
        assert_eq!(
            names(Filters::new(vec![], None, None, vec!["@smoke".into()]).unwrap()).len(),
            2
        );
        assert_eq!(
            names(Filters::new(vec![], None, None, vec!["fast".into(), "nope".into()]).unwrap()),
            ["login > greets"]
        );
        assert!(Filters::new(vec![], Some("("), None, vec![]).is_err());
    }

    fn schema_property_names(value: &Value, skip_defs: bool, names: &mut Vec<String>) {
        let Value::Object(map) = value else {
            if let Value::Array(items) = value {
                for item in items {
                    schema_property_names(item, skip_defs, names);
                }
            }
            return;
        };
        for (key, child) in map {
            match key.as_str() {
                "properties" => {
                    if let Value::Object(properties) = child {
                        names.extend(properties.keys().cloned());
                    }
                    schema_property_names(child, skip_defs, names);
                }
                "$defs" if skip_defs => {
                    if let Value::Object(defs) = child {
                        for (name, def) in defs {
                            // Gate and hook shapes belong to the legacy drill script format.
                            if !name.starts_with("gate") && !name.starts_with("hook") {
                                schema_property_names(def, skip_defs, names);
                            }
                        }
                    }
                }
                _ => schema_property_names(child, skip_defs, names),
            }
        }
    }

    #[test]
    fn schema_matches_parser_keys() {
        let schema: Value = serde_json::from_str(DRILL_SCHEMA).unwrap();
        assert_eq!(schema["$id"], DRILL_SCHEMA_URL);
        let mut names = Vec::new();
        schema_property_names(&schema, true, &mut names);
        let known = model::all_known_keys();
        for key in &known {
            assert!(
                names.iter().any(|name| name == key),
                "schema is missing `{key}`"
            );
        }
        for name in &names {
            assert!(
                known.contains(&name.as_str()),
                "parser does not know schema key `{name}`"
            );
        }
        for (key, keys) in [
            ("action", model::ACTION_KEYS),
            ("matcher", model::MATCHER_KEYS),
        ] {
            for item in keys {
                assert!(
                    DRILL_SCHEMA.contains(&format!("\"{item}\"")),
                    "schema is missing {key} `{item}`"
                );
            }
        }
    }

    /// Expected verdicts: Both = parser and schema agree; ParserOnly = only the
    /// parser can see the problem (regex validity, duplicate names).
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Verdict {
        Valid,
        Invalid,
        ParserOnly,
    }

    #[test]
    fn schema_and_parser_agree_on_fixtures() {
        use Verdict::*;
        let steps: &[(Verdict, Value)] = &[
            (Valid, json!({ "click": "#a" })),
            (Valid, json!({ "click": "#a", "button": "right" })),
            (Valid, json!({ "click": "#a", "button": "middle" })),
            (Valid, json!({ "click": "#a", "button": "left" })),
            (Invalid, json!({ "click": "#a", "button": "back" })),
            (Invalid, json!({ "click": "#a", "button": 2 })),
            (Invalid, json!({ "dblclick": "#a", "button": "right" })),
            (
                Valid,
                json!({ "click": { "role": "button", "name": "Go", "exact": true, "nth": -1 } }),
            ),
            (Valid, json!({ "fill": "#a", "value": "" })),
            (
                Valid,
                json!({ "type": { "testId": "name" }, "value": "Seth" }),
            ),
            (Valid, json!({ "select": "#s", "values": ["a", "b"] })),
            (Valid, json!({ "select": "#s", "value": "a" })),
            (Valid, json!({ "check": { "label": "Agree" } })),
            (Valid, json!({ "expect": "#o", "toHaveText": "" })),
            (
                Valid,
                json!({ "expect": "#o", "toHaveText": { "pattern": "^Hi" } }),
            ),
            (
                Valid,
                json!({ "expect": "#o", "toBeVisible": true, "not": true, "soft": true }),
            ),
            (Valid, json!({ "expect": "#o", "toHaveCount": 0 })),
            (
                Valid,
                json!({ "expect": "#o", "toHaveScreenshot": true, "maxDiffPixels": 3, "mask": ["#clock"] }),
            ),
            (
                Valid,
                json!({ "expect": "#o", "toHaveScreenshot": "home", "not": false }),
            ),
            (
                Valid,
                json!({ "expect": "#o", "toMatchAriaSnapshot": "home" }),
            ),
            (
                Valid,
                json!({ "expectIpc": "greet", "status": "error", "count": 1, "maxMs": 50 }),
            ),
            (Valid, json!({ "expectIpc": "greet", "not": true })),
            (
                Valid,
                json!({ "expectIpc": "greet", "not": false, "count": 2 }),
            ),
            (Valid, json!({ "expectEvent": "ready", "count": 1 })),
            (Valid, json!({ "expectLog": "hello", "level": "info" })),
            (
                Valid,
                json!({ "expectNoErrors": true, "ignore": ["ResizeObserver"] }),
            ),
            (
                Valid,
                json!({ "press": "Enter", "target": { "placeholder": "Name" } }),
            ),
            (
                Valid,
                json!({ "waitFor": { "text": "Hi", "exact": true }, "state": "hidden" }),
            ),
            (
                Valid,
                json!({ "gate": { "name": "g", "instructions": "do it", "manualContinue": true } }),
            ),
            (
                Valid,
                json!({ "gate": { "name": "g", "instructions": "do it", "selector": "#a", "expectText": "x" } }),
            ),
            (Valid, json!({ "sleep": 10, "name": "settle" })),
            (Valid, json!({ "reload": true })),
            (Valid, json!({ "screenshot": "home", "target": "#a" })),
            (Valid, json!({ "evaluate": "1 + 1", "timeoutMs": 100 })),
            (Invalid, json!({ "clik": "#a" })),
            (Invalid, json!({ "click": "" })),
            (Invalid, json!({ "click": "  " })),
            (Invalid, json!({ "click": "#a", "dblclick": "#b" })),
            (Invalid, json!({ "click": "#a", "value": "x" })),
            (Invalid, json!({ "click": { "css": "#a", "exact": true } })),
            (
                Invalid,
                json!({ "click": { "testId": "a", "exact": true } }),
            ),
            (
                Invalid,
                json!({ "click": { "role": "button", "exact": true } }),
            ),
            (Invalid, json!({ "click": { "text": "a", "name": "b" } })),
            (
                Invalid,
                json!({ "click": { "css": "#a", "role": "button" } }),
            ),
            (Invalid, json!({ "click": {} })),
            (Invalid, json!({ "fill": "#a" })),
            (
                Invalid,
                json!({ "select": "#s", "value": "a", "values": ["b"] }),
            ),
            (Invalid, json!({ "select": "#s" })),
            (Invalid, json!({ "expect": "#o" })),
            (
                Invalid,
                json!({ "expect": "#o", "toBeVisible": true, "toBeHidden": true }),
            ),
            (Invalid, json!({ "expect": "#o", "toBeVisible": false })),
            (
                Invalid,
                json!({ "expect": "#o", "toHaveScreenshot": true, "not": true }),
            ),
            (
                Invalid,
                json!({ "expect": "#o", "toMatchAriaSnapshot": true, "not": true }),
            ),
            (
                Invalid,
                json!({ "expect": "#o", "toBeVisible": true, "threshold": 0.1 }),
            ),
            (
                Invalid,
                json!({ "expect": "#o", "toHaveScreenshot": "../x" }),
            ),
            (Invalid, json!({ "expect": "#o", "toHaveScreenshot": ".x" })),
            (Invalid, json!({ "expect": "#o", "toHaveCount": -1 })),
            (
                Invalid,
                json!({ "expectIpc": "greet", "not": true, "count": 2 }),
            ),
            (Invalid, json!({ "expectIpc": "greet", "status": "bad" })),
            (Invalid, json!({ "expectEvent": "ready", "status": "ok" })),
            (Invalid, json!({ "expectNoErrors": false })),
            (Invalid, json!({ "sleep": 0 })),
            (Invalid, json!({ "sleep": 60001 })),
            (Invalid, json!({ "reload": false })),
            (Invalid, json!({ "press": " " })),
            (Invalid, json!({ "screenshot": true })),
            (
                Valid,
                json!({ "gate": { "name": "g", "instructions": "do it" } }),
            ),
            (
                Valid,
                json!({ "gate": { "name": "g", "instructions": "do it", "manualContinue": false, "choices": [{ "id": "a", "label": "A" }] } }),
            ),
            (
                Invalid,
                json!({ "gate": { "name": "g", "instructions": "do it", "manualContinue": false } }),
            ),
            (
                Invalid,
                json!({ "gate": { "name": "g", "instructions": "do it", "manualContinue": false, "choices": [] } }),
            ),
            (
                Invalid,
                json!({ "gate": { "name": "g", "instructions": "do it", "expectText": "x", "manualContinue": true } }),
            ),
            (
                Invalid,
                json!({ "gate": { "name": "g", "instructions": " ", "manualContinue": true } }),
            ),
            (Invalid, json!({ "click": "#a", "timeoutMs": 0 })),
            (Invalid, json!("click")),
            (
                ParserOnly,
                json!({ "expect": "#o", "toHaveText": { "pattern": "(" } }),
            ),
        ];
        let step = |value: &Value| json!({ "tests": [{ "name": "t", "steps": [value] }] });
        let ok = json!([{ "click": "#a" }]);
        let suites: Vec<(Verdict, Value)> = vec![
            (
                Valid,
                json!({ "tests": [{ "name": "a", "skip": "later" }] }),
            ),
            (Valid, json!({ "tests": [{ "name": "a", "fixme": true }] })),
            (
                Valid,
                json!({ "tests": [{ "name": "a", "only": true, "steps": ok }] }),
            ),
            (
                Valid,
                json!({ "tests": [{ "name": "a", "only": false, "skip": true }] }),
            ),
            (
                Valid,
                json!({ "tests": [{ "name": "a", "skip": false, "steps": ok }] }),
            ),
            (
                Valid,
                json!({ "tests": [{ "name": "a", "expectFail": "bug #1", "slow": true, "retries": 10, "steps": ok }] }),
            ),
            (
                Valid,
                json!({ "$schema": DRILL_SCHEMA_URL, "tags": ["@smoke", "@@a", "b"], "tests": [{ "name": "a", "steps": ok }] }),
            ),
            (
                Valid,
                json!({
                    "suite": "s", "description": "d", "timeoutMs": 5, "expectTimeoutMs": 5, "retries": 1,
                    "setup": [{ "name": "h", "run": "npm", "args": ["ci"], "timeoutMs": 10 }],
                    "beforeEach": ok, "afterAll": ok,
                    "tests": [{ "name": "a", "steps": ok }]
                }),
            ),
            (Invalid, json!({})),
            (Invalid, json!([])),
            (Invalid, json!({ "tests": [] })),
            (Invalid, json!({ "tests": [{ "name": "a" }] })),
            (Invalid, json!({ "tests": [{ "name": "a", "steps": [] }] })),
            (
                Invalid,
                json!({ "tests": [{ "name": "a", "skip": false }] }),
            ),
            (
                Invalid,
                json!({ "tests": [{ "name": "a", "only": true, "skip": true }] }),
            ),
            (
                Invalid,
                json!({ "tests": [{ "name": "a", "skip": true, "fixme": "x" }] }),
            ),
            (Invalid, json!({ "tests": [{ "name": "a", "skip": "" }] })),
            (Invalid, json!({ "tests": [{ "name": " ", "steps": ok }] })),
            (
                Invalid,
                json!({ "tests": [{ "name": "a", "retries": 11, "steps": ok }] }),
            ),
            (Invalid, json!({ "tests": [{ "steps": ok }] })),
            (
                Invalid,
                json!({ "$schema": 1, "tests": [{ "name": "a", "steps": ok }] }),
            ),
            (
                Invalid,
                json!({ "description": "", "tests": [{ "name": "a", "steps": ok }] }),
            ),
            (
                Invalid,
                json!({ "suite": "", "tests": [{ "name": "a", "steps": ok }] }),
            ),
            (
                Invalid,
                json!({ "tags": ["@"], "tests": [{ "name": "a", "steps": ok }] }),
            ),
            (
                Invalid,
                json!({ "tags": ["a b"], "tests": [{ "name": "a", "steps": ok }] }),
            ),
            (
                Invalid,
                json!({ "setup": [{ "name": "h" }], "tests": [{ "name": "a", "steps": ok }] }),
            ),
            (
                Invalid,
                json!({ "setup": [{ "name": "h", "run": "x", "timeoutMs": 0 }], "tests": [{ "name": "a", "steps": ok }] }),
            ),
            (
                Invalid,
                json!({ "bogus": 1, "tests": [{ "name": "a", "steps": ok }] }),
            ),
            (
                ParserOnly,
                json!({ "tests": [{ "name": "a", "steps": ok }, { "name": "a", "steps": ok }] }),
            ),
        ];

        let schema: Value = serde_json::from_str(DRILL_SCHEMA).unwrap();
        let validator = jsonschema::draft202012::new(&schema).unwrap();
        let mut mismatches = Vec::new();
        let cases = steps
            .iter()
            .map(|(verdict, value)| (*verdict, step(value)))
            .chain(suites);
        for (verdict, document) in cases {
            let text = document.to_string();
            let parsed = model::parse_str(Path::new("x.drill.json"), &text);
            let parser_valid = parsed.issues.is_empty();
            let schema_valid = validator.is_valid(&document);
            let (want_parser, want_schema) = match verdict {
                Valid => (true, true),
                Invalid => (false, false),
                ParserOnly => (false, true),
            };
            if parser_valid != want_parser || schema_valid != want_schema {
                mismatches.push(format!(
                    "{verdict:?}: parser={parser_valid} schema={schema_valid} {text}\n  issues: {:?}",
                    parsed.issues
                ));
            }
        }
        assert!(mismatches.is_empty(), "\n{}", mismatches.join("\n"));
    }

    #[test]
    fn only_focuses_selection_across_suites() {
        let parse = |name: &str, text: &str| {
            model::parse_str(Path::new(&format!("{name}.drill.json")), text)
                .suite
                .unwrap()
        };
        let suites = vec![
            parse(
                "a",
                r##"{ "tests": [{ "name": "x", "steps": [{ "click": "#a" }] }, { "name": "y", "only": true, "steps": [{ "click": "#a" }] }] }"##,
            ),
            parse(
                "b",
                r##"{ "tests": [{ "name": "z", "steps": [{ "click": "#a" }] }] }"##,
            ),
        ];
        let filters = Filters::new(vec![], None, None, vec![]).unwrap();
        let selected = select(&suites, &filters);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].tests.len(), 1);
        assert_eq!(selected[0].tests[0].name, "y");
        // Filters apply before focus, so an `only` test filtered out does not hide others.
        let filters = Filters::new(vec!["b".into()], None, None, vec![]).unwrap();
        assert_eq!(select(&suites, &filters)[0].tests[0].name, "z");
        if cfg!(windows) {
            let filters = Filters::new(vec!["B.DRILL".into()], None, None, vec![]).unwrap();
            assert_eq!(select(&suites, &filters).len(), 1);
        }
    }

    #[test]
    fn docs_schema_copy_is_identical() {
        let docs = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/public/schemas/drill.schema.json");
        // Packaged crates do not ship the docs site.
        if let Ok(copy) = std::fs::read_to_string(docs) {
            assert_eq!(
                copy.replace("\r\n", "\n"),
                DRILL_SCHEMA.replace("\r\n", "\n"),
                "copy crates/auditaur-cli/assets/drill.schema.json to docs/public/schemas/"
            );
        }
    }
}
