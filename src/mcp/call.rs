//! Runs one tool call as a child process of this same binary and maps its JSON envelope to an MCP result.

use clap::{CommandFactory, parser::ValueSource};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use super::calls::Calls;
use super::catalog::Tool;
use crate::cli::Cli;

const CANCELLED: &str = "Cancelled";

/// Runs `tool` with `arguments` for request `id`, carrying the server's own global flags, and returns a
/// `CallToolResult`. A cancelled call returns early; the caller sends no response for it.
pub(crate) fn call(
    tool: &Tool,
    arguments: &Map<String, Value>,
    globals: &[String],
    calls: &Calls,
    id: &Value,
) -> Value {
    let argv = match tool.argv(arguments) {
        Ok(argv) => argv,
        Err(message) => return error_result(&message),
    };
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => return error_result(&format!("Cannot locate agent-desktop: {error}")),
    };
    let mut command = child_command(&exe, tool, argv, globals);
    let (stdout, stderr) = match calls.start(id, &mut command) {
        None => return error_result(CANCELLED),
        Some(Err(error)) => return error_result(&format!("Cannot run agent-desktop: {error}")),
        Some(Ok(pipes)) => pipes,
    };
    let (stdout, stderr) = std::thread::scope(|scope| {
        let stderr = scope.spawn(|| read_all(stderr));
        (read_all(stdout), stderr.join().unwrap_or_default())
    });
    let Some(mut child) = calls.take(id) else {
        return error_result(CANCELLED);
    };
    let status = child
        .wait()
        .map_or_else(|error| error.to_string(), |status| status.to_string());
    match serde_json::from_slice::<Value>(&stdout) {
        Ok(envelope) => envelope_result(envelope),
        Err(_) => error_result(&format!(
            "agent-desktop {} exited with {status} and no JSON envelope: {}",
            tool.path.join(" "),
            String::from_utf8_lossy(&stderr).trim()
        )),
    }
}

/// Builds the child command that runs `tool`: server globals precede an ordinary tool, and belong to the
/// guest command after `sandbox exec NAME --`. A global the guest sets itself wins over the server's value
/// and over any server global that conflicts with it, and the guest's own parser remains the authority on
/// whether its full argument list is valid.
fn child_command(exe: &Path, tool: &Tool, argv: Vec<String>, globals: &[String]) -> Command {
    let mut command = Command::new(exe);
    command
        .args(command_args(tool, argv, globals))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn command_args(tool: &Tool, argv: Vec<String>, globals: &[String]) -> Vec<String> {
    if globals.is_empty() {
        return argv;
    }
    if tool.path != ["sandbox", "exec"] {
        return globals.iter().cloned().chain(argv).collect();
    }
    let Some(delimiter) = argv.iter().position(|arg| arg == "--") else {
        return globals.iter().cloned().chain(argv).collect();
    };
    let guest = &argv[delimiter + 1..];
    if guest.is_empty() {
        return argv;
    }
    let overridden = overridden_globals(guest);
    let mut args = argv[..=delimiter].to_vec();
    args.extend(
        globals
            .iter()
            .filter(|global| !overridden.contains(flag_name(global)))
            .cloned(),
    );
    args.extend(guest.iter().cloned());
    args
}

fn flag_name(global: &str) -> &str {
    let flag = global.strip_prefix("--").unwrap_or(global);
    flag.split('=').next().unwrap_or(flag)
}

/// The globals the guest command sets itself, plus every global that conflicts with one of them. Argument
/// errors are ignored for metadata only, and a parse that cannot supply matches yields an empty set so the
/// caller keeps every inherited global.
fn overridden_globals(guest: &[String]) -> BTreeSet<String> {
    let mut command = Cli::command()
        .subcommand(super::command())
        .ignore_errors(true);
    let argv = std::iter::once("agent-desktop")
        .chain(guest.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let Ok(matches) = command.try_get_matches_from_mut(argv) else {
        return BTreeSet::new();
    };
    let globals = command
        .get_arguments()
        .filter(|arg| arg.is_global_set())
        .collect::<Vec<_>>();
    let conflicts = |one: &clap::Arg, other: &clap::Arg| {
        command
            .get_arg_conflicts_with(one)
            .iter()
            .any(|conflict| conflict.get_id() == other.get_id())
    };
    let explicit = globals
        .iter()
        .filter(|arg| matches.value_source(arg.get_id().as_str()) == Some(ValueSource::CommandLine))
        .collect::<Vec<_>>();
    globals
        .iter()
        .filter(|arg| {
            explicit.iter().any(|set| {
                set.get_id() == arg.get_id() || conflicts(set, arg) || conflicts(arg, set)
            })
        })
        .filter_map(|arg| arg.get_long().map(ToString::to_string))
        .collect()
}

fn read_all(pipe: Option<impl Read>) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut bytes);
    }
    bytes
}

/// Maps an envelope to a result: the envelope as structured content and text, an image block for inline screenshots.
pub(crate) fn envelope_result(mut envelope: Value) -> Value {
    let is_error = envelope["ok"] != Value::Bool(true);
    let image = take_inline_image(&mut envelope);
    let mut content = vec![json!({ "type": "text", "text": envelope.to_string() })];
    content.extend(image);
    json!({ "content": content, "structuredContent": envelope, "isError": is_error })
}

fn take_inline_image(envelope: &mut Value) -> Option<Value> {
    let data = envelope.get_mut("data")?.as_object_mut()?;
    let mime_type = match data.get("format")?.as_str()? {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        _ => return None,
    };
    if !data.get("data").is_some_and(Value::is_string) {
        return None;
    }
    let Value::String(encoded) = data.remove("data")? else {
        return None;
    };
    Some(json!({ "type": "image", "data": encoded, "mimeType": mime_type }))
}

fn error_result(message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

#[cfg(test)]
#[path = "call_tests.rs"]
mod tests;
