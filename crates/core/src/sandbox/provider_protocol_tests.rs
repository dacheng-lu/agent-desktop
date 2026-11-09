use serde_json::json;

use crate::sandbox::provider_protocol::{exec_outcome, parse_reply};
use crate::sandbox::provider_script_output::ProviderScriptOutput;
use crate::{AdapterError, AppError, DeliverySemantics, ErrorCode};

const READY_LINE: &str = r#"{"ok":true,"state":"ready","message":"The dev sandbox is ready.","next":"agent-desktop sandbox exec dev -- snapshot","data":{"changed":true}}"#;
const UNREADABLE: &str = "The macos-vm provider returned output agent-desktop could not read.";
const NEXT_SUGGESTION: &str = "Run `agent-desktop sandbox list` to see your sandboxes.";

fn output(stdout: &str, stderr: &str) -> ProviderScriptOutput {
    ProviderScriptOutput {
        status: Some(1),
        stdout: stdout.as_bytes().to_vec(),
        stderr_tail: stderr.to_owned(),
    }
}

fn failure(code: &str, message: &str, suggestion: &str) -> String {
    json!({
        "ok": false,
        "code": code,
        "message": message,
        "suggestion": suggestion,
    })
    .to_string()
}

fn adapter(error: AppError) -> AdapterError {
    match error {
        AppError::Adapter(error) => error,
        other => panic!("expected an adapter error, got {other:?}"),
    }
}

fn failed_reply(provider: &str, name: Option<&str>, mutating: bool, stdout: &str) -> AdapterError {
    adapter(parse_reply(provider, name, mutating, &output(stdout, "")).unwrap_err())
}

#[test]
fn unreadable_provider_output_is_action_failed_with_stderr_out_of_the_message() {
    let stderr = "Error: image build failed\nexit status 1";
    let unreadable = output("starting the build...\nnot json at all\n", stderr);
    let error = adapter(parse_reply("macos-vm", Some("dev"), false, &unreadable).unwrap_err());

    assert_eq!(error.code, ErrorCode::ActionFailed);
    assert_eq!(error.message, UNREADABLE);
    assert_eq!(error.platform_detail.as_deref(), Some(stderr));
    assert!(!error.message.contains("image build failed"));

    for stdout in [
        r#"{"ok":true}"#,
        r#"{"ok":true,"state":"parked","message":"The dev sandbox is ready."}"#,
        r#"{"ok":false,"code":"ACTION_FAILED","message":"The macos-vm provider stopped."}"#,
        "[1,2,3]",
    ] {
        let error = failed_reply("macos-vm", Some("dev"), false, stdout);

        assert_eq!(error.code, ErrorCode::ActionFailed, "{stdout}");
        assert_eq!(error.message, UNREADABLE, "{stdout}");
    }
}

#[test]
fn a_failed_verb_reports_the_delivery_it_cannot_verify() {
    let failed_mutation = failure(
        "ACTION_FAILED",
        "The macos-vm provider could not start dev.",
        NEXT_SUGGESTION,
    );
    assert_eq!(
        failed_reply("macos-vm", Some("dev"), true, &failed_mutation).disposition,
        DeliverySemantics::unknown()
    );

    let unchanged = json!({
        "ok": false,
        "code": "ACTION_FAILED",
        "message": "The macos-vm provider could not start dev.",
        "suggestion": NEXT_SUGGESTION,
        "data": { "changed": false },
    })
    .to_string();
    assert_eq!(
        failed_reply("macos-vm", Some("dev"), true, &unchanged).disposition,
        DeliverySemantics::not_delivered()
    );

    let failed_read = failure(
        "TIMEOUT",
        "The macos-vm provider did not answer.",
        NEXT_SUGGESTION,
    );
    assert_eq!(
        failed_reply("macos-vm", Some("dev"), false, &failed_read).disposition,
        DeliverySemantics::not_delivered()
    );
}

#[test]
fn exec_passes_a_guest_envelope_through_byte_for_byte() {
    let mut guest =
        br#"{"version":"2.5","ok":false,"command":"snapshot","error":{"code":"STALE_REF"}}"#
            .to_vec();
    guest.push(b'\n');
    guest.push(0xff);
    let output = ProviderScriptOutput {
        status: Some(1),
        stdout: guest.clone(),
        stderr_tail: "the guest refused".to_owned(),
    };

    let passthrough = exec_outcome("macos-vm", &output).unwrap();

    assert_eq!(passthrough.stdout, guest);
    assert_eq!(passthrough.status, 1);
}

#[test]
fn exec_failures_report_the_sandbox_error_and_unknown_delivery() {
    let provider_failure = failure(
        "SANDBOX_NOT_FOUND",
        "The sandbox dev does not exist.",
        NEXT_SUGGESTION,
    );
    let output = ProviderScriptOutput {
        status: Some(125),
        stdout: provider_failure.into_bytes(),
        stderr_tail: String::new(),
    };
    let error = adapter(exec_outcome("macos-vm", &output).unwrap_err());
    assert_eq!(error.code, ErrorCode::SandboxNotFound);
    assert_eq!(error.message, "The sandbox dev does not exist.");
    assert_eq!(error.disposition, DeliverySemantics::unknown());

    let unreadable = ProviderScriptOutput {
        status: Some(125),
        stdout: READY_LINE.as_bytes().to_vec(),
        stderr_tail: "ssh: connect to host failed".to_owned(),
    };
    let error = adapter(exec_outcome("macos-vm", &unreadable).unwrap_err());
    assert_eq!(error.code, ErrorCode::ActionFailed);
    assert_eq!(error.message, UNREADABLE);
    assert_eq!(
        error.platform_detail.as_deref(),
        Some("ssh: connect to host failed")
    );

    let no_status = ProviderScriptOutput {
        status: None,
        stdout: Vec::new(),
        stderr_tail: "ssh died".to_owned(),
    };
    let error = adapter(exec_outcome("macos-vm", &no_status).unwrap_err());
    assert_eq!(error.code, ErrorCode::ActionFailed);
    assert_eq!(error.disposition, DeliverySemantics::unknown());
}
