use super::execute;
use crate::commands::sandbox_action::SandboxAction;
use crate::refs::refs_test_support::HomeGuard;
use crate::sandbox::provider_test_support::ScriptedAdapter;
use crate::sandbox::sandbox_record::SandboxRecord;
use crate::sandbox::sandbox_store;
use crate::{AdapterError, AppError, DeliverySemantics, ErrorCode};
use std::path::{Path, PathBuf};

const STOPPED: &str = r#"{"ok":true,"state":"stopped","message":"Sandbox dev-1 is stopped."}"#;
const RUNNING: &str = r#"{"ok":true,"state":"running","message":"Sandbox dev-1 is running."}"#;
const REMOVED: &str = r#"{"ok":true,"state":"stopped","message":"Sandbox dev-1 is removed."}"#;
const UNREADABLE: &str = "the provider printed no reply";
const RM_FAILED: &str = r#"{"ok":false,"code":"ACTION_FAILED","message":"The provider did not remove the sandbox.","suggestion":"Run `agent-desktop sandbox rm dev-1` again."}"#;

fn rm_action() -> SandboxAction {
    SandboxAction::Rm {
        name: "dev-1".to_string(),
        provider: None,
    }
}

fn rm_recovery_action(provider: &str) -> SandboxAction {
    SandboxAction::Rm {
        name: "dev-1".to_string(),
        provider: Some(provider.to_string()),
    }
}

fn adapter_error(error: AppError) -> AdapterError {
    match error {
        AppError::Adapter(error) => error,
        other => panic!("expected an adapter error, got {other:?}"),
    }
}

fn sandboxes_root(home: &HomeGuard) -> PathBuf {
    home.path().join(".agent-desktop").join("sandboxes")
}

fn record(name: &str, provider: &str, created_at: u64) -> SandboxRecord {
    SandboxRecord {
        name: name.to_string(),
        provider: provider.to_string(),
        created_at,
    }
}

fn write_record(root: &Path, name: &str, bytes: &[u8]) {
    crate::storage::private_file::write_atomic(
        &root.join(name).join(sandbox_store::RECORD_FILE),
        bytes,
    )
    .expect("a private record write");
}

fn verbs(adapter: &ScriptedAdapter) -> Vec<String> {
    adapter
        .requests()
        .into_iter()
        .map(|request| request.args.first().cloned().unwrap_or_default())
        .collect()
}

#[test]
fn a_malformed_record_is_listed_as_broken_without_a_provider_and_with_recovery() {
    let home = HomeGuard::new();
    let root = sandboxes_root(&home);
    sandbox_store::create(&root, &record("broken", "stub", 1)).expect("broken record");
    sandbox_store::create(&root, &record("healthy", "stub", 2)).expect("healthy record");
    write_record(&root, "broken", b"{not json");
    let adapter = ScriptedAdapter::new().reply("status", STOPPED, 0);

    let listed = execute(SandboxAction::List, &adapter).expect("list");
    let broken = &listed["sandboxes"][0];
    let healthy = &listed["sandboxes"][1];

    assert_eq!(broken["name"], "broken");
    assert_eq!(broken["state"], "broken");
    assert!(
        broken.get("provider").is_none(),
        "a record that cannot be read supplies no provider"
    );
    assert!(
        broken["next"]
            .as_str()
            .is_some_and(|next| next.contains("--provider")),
        "the broken entry names the recovery flag"
    );
    assert_eq!(healthy["name"], "healthy");
    assert_eq!(healthy["provider"], "stub");
    assert_eq!(healthy["state"], "stopped");
}

#[test]
fn a_missing_record_is_listed_with_recovery_guidance() {
    let home = HomeGuard::new();
    let root = sandboxes_root(&home);
    sandbox_store::create(&root, &record("dev-1", "stub", 1)).expect("record");
    std::fs::remove_file(root.join("dev-1").join(sandbox_store::RECORD_FILE))
        .expect("remove record");

    let listed = execute(SandboxAction::List, &ScriptedAdapter::new()).expect("list");
    let broken = &listed["sandboxes"][0];

    assert_eq!(broken["state"], "broken");
    assert!(broken.get("provider").is_none());
    assert!(
        broken["message"]
            .as_str()
            .is_some_and(|message| message.contains("missing")),
        "the entry says the record file is missing"
    );
    assert!(
        broken["next"]
            .as_str()
            .is_some_and(|next| next.contains("--provider")),
        "the entry names the recovery flag"
    );
}

#[test]
fn removing_a_malformed_record_without_a_provider_returns_an_actionable_error() {
    let home = HomeGuard::new();
    let root = sandboxes_root(&home);
    sandbox_store::create(&root, &record("dev-1", "stub", 1)).expect("record");
    write_record(&root, "dev-1", b"{not json");
    let adapter = ScriptedAdapter::new();

    let error = adapter_error(execute(rm_action(), &adapter).expect_err("no provider to use"));

    assert_eq!(error.code, ErrorCode::InvalidArgs);
    assert!(error.message.contains("dev-1"));
    assert!(
        error
            .suggestion
            .as_deref()
            .is_some_and(|suggestion| suggestion.contains("--provider")),
        "the suggestion names the recovery flag"
    );
    assert_eq!(error.disposition, DeliverySemantics::not_delivered());
    assert!(adapter.requests().is_empty(), "no provider was guessed");
    assert!(root.join("dev-1").exists(), "the sandbox data is untouched");
}

#[test]
fn an_explicit_provider_recovers_a_malformed_record_through_the_provider() {
    let home = HomeGuard::new();
    let root = sandboxes_root(&home);
    sandbox_store::create(&root, &record("dev-1", "stub", 1)).expect("record");
    write_record(&root, "dev-1", b"{not json");
    let adapter = ScriptedAdapter::new()
        .reply("status", STOPPED, 0)
        .reply("rm", REMOVED, 0);

    execute(rm_recovery_action("stub"), &adapter).expect("recovery");

    assert_eq!(
        verbs(&adapter),
        vec!["status".to_string(), "rm".to_string()]
    );
    assert!(
        !root.join("dev-1").exists(),
        "the provider rm removes the data"
    );
}

#[test]
fn an_explicit_provider_recovers_a_sandbox_directory_without_a_record() {
    let home = HomeGuard::new();
    let root = sandboxes_root(&home);
    sandbox_store::create(&root, &record("dev-1", "stub", 1)).expect("record");
    std::fs::remove_file(root.join("dev-1").join(sandbox_store::RECORD_FILE))
        .expect("remove record");
    let adapter = ScriptedAdapter::new()
        .reply("status", STOPPED, 0)
        .reply("rm", REMOVED, 0);

    execute(rm_recovery_action("stub"), &adapter).expect("recovery");

    assert_eq!(
        verbs(&adapter),
        vec!["status".to_string(), "rm".to_string()]
    );
    assert!(!root.join("dev-1").exists());
}

#[test]
fn a_healthy_record_refuses_a_different_provider() {
    let home = HomeGuard::new();
    let root = sandboxes_root(&home);
    sandbox_store::create(&root, &record("dev-1", "stub", 1)).expect("record");
    let adapter = ScriptedAdapter::new();

    let error =
        adapter_error(execute(rm_recovery_action("other"), &adapter).expect_err("mismatch"));

    assert_eq!(error.code, ErrorCode::InvalidArgs);
    assert!(error.message.contains("stub"));
    assert!(adapter.requests().is_empty(), "the provider never ran");
    assert!(root.join("dev-1").exists());
}

#[test]
fn an_unknown_provider_is_refused_before_the_provider_runs() {
    let home = HomeGuard::new();
    let root = sandboxes_root(&home);
    sandbox_store::create(&root, &record("dev-1", "stub", 1)).expect("record");
    write_record(&root, "dev-1", b"{not json");
    let adapter = ScriptedAdapter::new();

    let error = adapter_error(execute(rm_recovery_action("nomad"), &adapter).expect_err("unknown"));

    assert_eq!(error.code, ErrorCode::InvalidArgs);
    assert!(adapter.requests().is_empty());
    assert!(root.join("dev-1").exists());
}

#[test]
fn recovery_refuses_a_confirmed_running_sandbox_and_keeps_it() {
    let home = HomeGuard::new();
    let root = sandboxes_root(&home);
    sandbox_store::create(&root, &record("dev-1", "stub", 1)).expect("record");
    write_record(&root, "dev-1", b"{not json");
    let adapter = ScriptedAdapter::new().reply("status", RUNNING, 0);

    let error = adapter_error(execute(rm_recovery_action("stub"), &adapter).expect_err("running"));

    assert_eq!(error.code, ErrorCode::InvalidArgs);
    assert_eq!(verbs(&adapter), vec!["status".to_string()]);
    assert!(root.join("dev-1").exists());
}

#[test]
fn recovery_keeps_the_data_when_the_provider_rm_fails() {
    let home = HomeGuard::new();
    let root = sandboxes_root(&home);
    sandbox_store::create(&root, &record("dev-1", "stub", 1)).expect("record");
    write_record(&root, "dev-1", b"{not json");
    let adapter = ScriptedAdapter::new()
        .reply("status", UNREADABLE, 0)
        .reply("rm", RM_FAILED, 0);

    let error = adapter_error(
        execute(rm_recovery_action("stub"), &adapter).expect_err("provider rm fails"),
    );

    assert_eq!(error.code, ErrorCode::ActionFailed);
    assert_eq!(
        verbs(&adapter),
        vec!["status".to_string(), "rm".to_string()]
    );
    assert!(root.join("dev-1").join(sandbox_store::RECORD_FILE).exists());
}

struct DeniedRecordRead(PathBuf);

impl crate::PrivateFileOps for DeniedRecordRead {
    fn read_private_bounded(&self, path: &Path, max_bytes: u64) -> std::io::Result<Vec<u8>> {
        if path == self.0.as_path() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "private record refused",
            ));
        }
        crate::storage::private_file::read_private_bounded_portable(path, max_bytes)
    }
}

#[test]
fn recovery_does_not_bypass_private_file_checks() {
    let home = HomeGuard::new();
    let root = sandboxes_root(&home);
    sandbox_store::create(&root, &record("dev-1", "stub", 1)).expect("record");
    let record = root.join("dev-1").join(sandbox_store::RECORD_FILE);
    let adapter = ScriptedAdapter::new();

    let error = crate::storage::private_file_ops::with_test_ops_override(
        std::rc::Rc::new(DeniedRecordRead(record)),
        || execute(rm_recovery_action("stub"), &adapter),
    )
    .expect_err("private file refusal");

    assert_eq!(error.code(), "PERM_DENIED");
    assert!(adapter.requests().is_empty(), "the provider never ran");
    assert!(root.join("dev-1").exists());
}
