//! Compile-fail UI tests proving `auditaur_command` emits its own friendly
//! diagnostic (instead of Tauri's opaque error) when applied to an async
//! command that does not return `Result`.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/auditaur_command_async_non_result.rs");
}
