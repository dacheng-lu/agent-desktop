use super::*;
use clap::Parser;
use serde_json::json;
use std::path::Path;

fn tool_at(path: &[&str]) -> Tool {
    super::super::catalog::tools()
        .into_iter()
        .find(|tool| tool.path == path)
        .expect("tool exists")
}

fn child_args(tool: &Tool, arguments: Value, globals: &[&str]) -> Vec<String> {
    let arguments = arguments.as_object().expect("arguments object");
    let argv = tool.argv(arguments).expect("arguments build argv");
    let globals = globals.iter().map(ToString::to_string).collect::<Vec<_>>();
    child_command(Path::new("agent-desktop"), tool, argv, &globals)
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

fn guest_args(args: &[String]) -> &[String] {
    let delimiter = args
        .iter()
        .position(|arg| arg == "--")
        .expect("guest delimiter");
    &args[delimiter + 1..]
}

#[test]
fn ordinary_tools_keep_server_globals_before_their_command() {
    let tool = tool_at(&["snapshot"]);
    let args = child_args(
        &tool,
        json!({ "app": "Finder" }),
        &["--session=abc", "--headed"],
    );
    assert_eq!(
        args,
        ["--session=abc", "--headed", "snapshot", "--app=Finder"]
    );
}

#[test]
fn sandbox_exec_applies_server_globals_inside_the_guest_command() {
    let tool = tool_at(&["sandbox", "exec"]);
    let args = child_args(
        &tool,
        json!({ "name": "dev", "args": ["snapshot", "--app", "Finder"] }),
        &["--session=abc", "--headed", "--trace=/tmp/guest"],
    );
    assert_eq!(
        args,
        [
            "sandbox",
            "exec",
            "dev",
            "--",
            "--session=abc",
            "--headed",
            "--trace=/tmp/guest",
            "snapshot",
            "--app",
            "Finder",
        ]
    );
}

#[test]
fn sandbox_exec_without_guest_arguments_keeps_the_schema_error() {
    let tool = tool_at(&["sandbox", "exec"]);
    let args = child_args(
        &tool,
        json!({ "name": "dev", "args": [] }),
        &["--session=abc"],
    );
    assert_eq!(args, ["sandbox", "exec", "dev", "--"]);
}

#[test]
fn sandbox_exec_guest_globals_win_without_duplicating_server_flags() {
    let guest = ["--session=guest", "--headed", "snapshot", "--app", "Finder"];
    let parsed = Cli::try_parse_from(std::iter::once("agent-desktop").chain(guest))
        .expect("guest arguments parse");
    assert_eq!(parsed.identity.session.as_deref(), Some("guest"));
    assert!(parsed.interaction.headed);
    let tool = tool_at(&["sandbox", "exec"]);
    let args = child_args(
        &tool,
        json!({ "name": "dev", "args": guest }),
        &[
            "--session=server",
            "--headed",
            "--trace=/tmp/server",
            "--wait-for=AXButton:Save",
        ],
    );
    assert_eq!(
        args,
        [
            "sandbox",
            "exec",
            "dev",
            "--",
            "--trace=/tmp/server",
            "--wait-for=AXButton:Save",
            "--session=guest",
            "--headed",
            "snapshot",
            "--app",
            "Finder",
        ]
    );
}

#[test]
fn sandbox_exec_recognizes_a_guest_short_global() {
    let guest = ["-v", "snapshot", "--app", "Finder"];
    let parsed = Cli::try_parse_from(std::iter::once("agent-desktop").chain(guest))
        .expect("guest arguments parse");
    assert!(parsed.visual_debug.verbose);
    let tool = tool_at(&["sandbox", "exec"]);
    let args = child_args(
        &tool,
        json!({ "name": "dev", "args": guest }),
        &["--verbose", "--session=abc"],
    );
    assert_eq!(
        args,
        [
            "sandbox",
            "exec",
            "dev",
            "--",
            "--session=abc",
            "-v",
            "snapshot",
            "--app",
            "Finder",
        ]
    );
}

#[test]
fn sandbox_exec_keeps_guest_delimiters_and_literal_arguments_in_order() {
    let tool = tool_at(&["sandbox", "exec"]);
    let args = child_args(
        &tool,
        json!({ "name": "dev", "args": ["type", "--", "@s8f3k2p9:e1", "--session=guest"] }),
        &["--session=server"],
    );
    assert_eq!(
        args,
        [
            "sandbox",
            "exec",
            "dev",
            "--",
            "--session=server",
            "type",
            "--",
            "@s8f3k2p9:e1",
            "--session=guest",
        ]
    );
}

#[test]
fn sandbox_exec_flag_shaped_guest_values_keep_the_server_session() {
    let guests = [
        vec![
            "type".to_string(),
            "--".to_string(),
            "@s8f3k2p9:e1".to_string(),
            "--session=literal".to_string(),
        ],
        vec![
            "launch".to_string(),
            "--arg".to_string(),
            "--session=literal".to_string(),
            "--".to_string(),
            "Finder".to_string(),
        ],
    ];
    let tool = tool_at(&["sandbox", "exec"]);
    for guest in guests {
        let parsed = Cli::try_parse_from(
            std::iter::once("agent-desktop").chain(guest.iter().map(String::as_str)),
        )
        .expect("guest arguments parse");
        assert!(parsed.identity.session.is_none(), "{guest:?}");
        let args = child_args(
            &tool,
            json!({ "name": "dev", "args": guest.clone() }),
            &["--session=server"],
        );
        let mut expected = Vec::from([
            "sandbox".to_string(),
            "exec".to_string(),
            "dev".to_string(),
            "--".to_string(),
            "--session=server".to_string(),
        ]);
        expected.extend(guest);
        assert_eq!(args, expected);
    }
}

#[test]
fn sandbox_exec_nested_inner_globals_stay_guest_data() {
    let guest = [
        "sandbox",
        "exec",
        "inner",
        "--",
        "--session=inner",
        "snapshot",
        "--app",
        "Finder",
    ];
    let parsed = Cli::try_parse_from(std::iter::once("agent-desktop").chain(guest))
        .expect("nested sandbox exec arguments parse");
    assert!(parsed.identity.session.is_none());
    let args = child_args(
        &tool_at(&["sandbox", "exec"]),
        json!({ "name": "outer", "args": guest }),
        &["--session=server"],
    );
    let mut expected = Vec::from([
        "sandbox".to_string(),
        "exec".to_string(),
        "outer".to_string(),
        "--".to_string(),
        "--session=server".to_string(),
    ]);
    expected.extend(guest.iter().map(ToString::to_string));
    assert_eq!(args, expected);
}

#[test]
fn sandbox_exec_guest_debug_inherits_the_server_screenshot() {
    let args = child_args(
        &tool_at(&["sandbox", "exec"]),
        json!({ "name": "dev", "args": ["--debug", "snapshot"] }),
        &["--debug", "--screenshot=/tmp/debug.html"],
    );
    assert_eq!(
        args,
        [
            "sandbox",
            "exec",
            "dev",
            "--",
            "--screenshot=/tmp/debug.html",
            "--debug",
            "snapshot",
        ]
    );
    let parsed = Cli::try_parse_from(
        std::iter::once("agent-desktop").chain(guest_args(&args).iter().map(String::as_str)),
    )
    .expect("guest with the inherited screenshot parses");
    assert!(parsed.visual_debug.debug);
    assert_eq!(
        parsed.visual_debug.screenshot.as_deref(),
        Some(Path::new("/tmp/debug.html"))
    );
}

#[test]
fn sandbox_exec_explicit_session_keeps_parent_flags_for_an_incomplete_guest() {
    let args = child_args(
        &tool_at(&["sandbox", "exec"]),
        json!({ "name": "dev", "args": ["click", "--session=guest"] }),
        &["--session=server", "--headed", "--trace=/tmp/trace"],
    );
    assert_eq!(
        args,
        [
            "sandbox",
            "exec",
            "dev",
            "--",
            "--headed",
            "--trace=/tmp/trace",
            "click",
            "--session=guest",
        ]
    );
    let error = Cli::try_parse_from(
        std::iter::once("agent-desktop").chain(guest_args(&args).iter().map(String::as_str)),
    )
    .expect_err("the missing ref is still invalid");
    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn sandbox_exec_invalid_guest_arguments_keep_parent_flags_for_the_guest_validator() {
    let guest = ["snapshot", "--session=guest", "--no-such-option"];
    let args = child_args(
        &tool_at(&["sandbox", "exec"]),
        json!({ "name": "dev", "args": guest }),
        &["--session=server", "--headed"],
    );
    assert_eq!(
        args,
        [
            "sandbox",
            "exec",
            "dev",
            "--",
            "--headed",
            "snapshot",
            "--session=guest",
            "--no-such-option",
        ]
    );
    let error = Cli::try_parse_from(std::iter::once("agent-desktop").chain(guest))
        .expect_err("invalid guest arguments");
    assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
}
