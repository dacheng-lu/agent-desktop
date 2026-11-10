use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::sandbox_action::SandboxAction;
use crate::sandbox::exec_passthrough::ExecPassthrough;
use crate::sandbox::provider_dirs::{SANDBOXES_DIR, resolve};
use crate::sandbox::provider_protocol::{exec_outcome, parse_reply, provider_refused};
use crate::sandbox::provider_reply::ProviderReply;
use crate::sandbox::provider_run::{ProviderCall, success_value};
use crate::sandbox::provider_settings::parse_settings;
use crate::sandbox::provider_spec::ProviderSpec;
use crate::sandbox::provider_table::find_provider;
use crate::sandbox::sandbox_name::validate_sandbox_name;
use crate::sandbox::sandbox_record::SandboxRecord;
use crate::sandbox::sandbox_state::SandboxState;
use crate::sandbox::sandbox_store;
use crate::storage::state_root::resolve_configured_state_root;
use crate::{AdapterError, AppError, DeliverySemantics, ErrorCode, PlatformAdapter};

const CREATE_TIMEOUT: Duration = Duration::from_secs(60);
const START_TIMEOUT: Duration = Duration::from_secs(60);
const STOP_TIMEOUT: Duration = Duration::from_secs(30);
const STATUS_TIMEOUT: Duration = Duration::from_secs(30);
const REMOVE_TIMEOUT: Duration = Duration::from_secs(30);
const EXEC_TIMEOUT: Duration = Duration::from_secs(300);

pub fn execute(action: SandboxAction, adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let root = resolve_configured_state_root()?;
    match action {
        SandboxAction::Install { provider } => super::sandbox_install::execute(provider, adapter),
        SandboxAction::Create {
            name,
            provider,
            set,
        } => create(&root, &name, &provider, &set, adapter),
        SandboxAction::Start { name } => verb_value(&root, &name, "start", START_TIMEOUT, adapter),
        SandboxAction::Stop { name } => verb_value(&root, &name, "stop", STOP_TIMEOUT, adapter),
        SandboxAction::Status { name } => {
            verb_value(&root, &name, "status", STATUS_TIMEOUT, adapter)
        }
        SandboxAction::List => list(&root, adapter),
        SandboxAction::Rm { name, provider } => remove(&root, &name, provider.as_deref(), adapter),
        SandboxAction::Exec { name, .. } => Err(exec_without_envelope(&name)),
    }
}

pub fn execute_exec(
    name: String,
    args: Vec<String>,
    adapter: &dyn PlatformAdapter,
) -> Result<ExecPassthrough, AppError> {
    let root = resolve_configured_state_root()?;
    let (spec, provider_dir, sandboxes_root) = locate(&root, &name)?;
    let call = ProviderCall::new(spec, &provider_dir, &sandboxes_root, Some(&name), &[]);
    let mut guest_args = vec!["exec".to_string(), name.clone(), "--".to_string()];
    guest_args.extend(args);
    let output = call.run(adapter, guest_args, EXEC_TIMEOUT)?;
    exec_outcome(spec.name, &output)
}

fn create(
    root: &Path,
    name: &str,
    provider: &str,
    set: &[String],
    adapter: &dyn PlatformAdapter,
) -> Result<Value, AppError> {
    validate_sandbox_name(name)?;
    let spec = find_provider(provider)?;
    let settings = parse_settings(set)?;
    let (provider_dir, sandboxes_root) = resolve(root, spec)?;
    sandbox_store::create(
        &sandboxes_root,
        &SandboxRecord {
            name: name.to_string(),
            provider: provider.to_string(),
            created_at: unix_seconds(),
        },
    )?;
    let call = ProviderCall::new(spec, &provider_dir, &sandboxes_root, Some(name), &settings);
    let output = call
        .run(adapter, verb_args(name, "create"), CREATE_TIMEOUT)
        .inspect_err(|error| {
            if error.code() != ErrorCode::Timeout.as_str() {
                discard_untouched_record(&sandboxes_root, name);
            }
        })?;
    let refused = provider_refused(&output);
    parse_reply(spec.name, Some(name), true, &output)
        .and_then(|reply| {
            if reply.state == SandboxState::Unavailable {
                discard_untouched_record(&sandboxes_root, name);
                Err(unavailable_refusal(reply.message))
            } else {
                Ok(success_value(name, Some(spec.name), reply))
            }
        })
        .inspect_err(|_| {
            if refused {
                discard_untouched_record(&sandboxes_root, name);
            }
        })
}

/// Drops the record of a create the provider refused. Every other failed create keeps its record, because
/// the provider may have made something that only `rm` can clear.
fn discard_untouched_record(sandboxes_root: &Path, name: &str) {
    let dir = sandboxes_root.join(name);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let untouched = entries
        .filter_map(Result::ok)
        .all(|entry| entry.file_name() == sandbox_store::RECORD_FILE);
    if untouched {
        let _ = sandbox_store::remove(sandboxes_root, name);
    }
}

fn verb_value(
    root: &Path,
    name: &str,
    verb: &str,
    timeout: Duration,
    adapter: &dyn PlatformAdapter,
) -> Result<Value, AppError> {
    let (spec, provider_dir, sandboxes_root) = locate(root, name)?;
    let call = ProviderCall::new(spec, &provider_dir, &sandboxes_root, Some(name), &[]);
    let output = call.run(adapter, verb_args(name, verb), timeout)?;
    let reply = parse_reply(spec.name, Some(name), is_mutating(verb), &output)?;
    Ok(success_value(name, Some(spec.name), reply))
}

fn remove(
    root: &Path,
    name: &str,
    provider: Option<&str>,
    adapter: &dyn PlatformAdapter,
) -> Result<Value, AppError> {
    validate_sandbox_name(name)?;
    let (spec, provider_dir, sandboxes_root) = match provider {
        Some(provider) => recovery_target(root, name, provider)?,
        None => locate(root, name)?,
    };
    let call = ProviderCall::new(spec, &provider_dir, &sandboxes_root, Some(name), &[]);
    let running = call
        .run(adapter, verb_args(name, "status"), STATUS_TIMEOUT)
        .and_then(|output| parse_reply(spec.name, Some(name), false, &output))
        .is_ok_and(|reply| reply.state == SandboxState::Running);
    if running {
        return Err(running_refusal(name));
    }
    let output = call.run(adapter, verb_args(name, "rm"), REMOVE_TIMEOUT)?;
    let reply = parse_reply(spec.name, Some(name), true, &output)?;
    sandbox_store::remove(&sandboxes_root, name)?;
    Ok(success_value(name, Some(spec.name), reply))
}

fn recovery_target(
    root: &Path,
    name: &str,
    provider: &str,
) -> Result<(&'static ProviderSpec, PathBuf, PathBuf), AppError> {
    let spec = find_provider(provider)?;
    let (provider_dir, sandboxes_root) = resolve(root, spec)?;
    match sandbox_store::load(&sandboxes_root, name) {
        Ok(record) if record.provider == provider => {}
        Ok(record) if find_provider(&record.provider).is_ok() => {
            return Err(provider_mismatch(name, &record.provider, provider));
        }
        Ok(_) => require_sandbox_dir(&sandboxes_root, name)?,
        Err(error) if recoverable_record(&error) => require_sandbox_dir(&sandboxes_root, name)?,
        Err(error) => return Err(error),
    }
    Ok((spec, provider_dir, sandboxes_root))
}

fn recoverable_record(error: &AppError) -> bool {
    error.code() == ErrorCode::SandboxNotFound.as_str()
        || error.code() == ErrorCode::InvalidArgs.as_str()
}

fn require_sandbox_dir(sandboxes_root: &Path, name: &str) -> Result<(), AppError> {
    let dir = sandboxes_root.join(name);
    match std::fs::symlink_metadata(&dir) {
        Ok(metadata) if metadata.is_dir() => {
            crate::storage::private_file_parent::ensure_private(&dir)?;
            Ok(())
        }
        Ok(_) => Err(AppError::invalid_input_with_suggestion(
            format!("Sandbox {name} is not a regular directory, so it was not recovered."),
            "Inspect the sandbox directory, then remove anything unexpected before retrying.",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Err(sandbox_store::not_found(name)),
        Err(error) => Err(error.into()),
    }
}

fn provider_mismatch(name: &str, recorded: &str, requested: &str) -> AppError {
    AppError::invalid_input_with_suggestion(
        format!(
            "Sandbox {name} was created with provider {recorded}, not {requested}, so it was not removed."
        ),
        format!("Run `agent-desktop sandbox rm {name}` to remove it with its recorded provider."),
    )
}

fn list(root: &Path, adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let entries = sandbox_store::list(&root.join(SANDBOXES_DIR))?;
    let mut sandboxes = Vec::with_capacity(entries.len());
    for entry in &entries {
        match &entry.record {
            Ok(record) => sandboxes.push(record_entry(root, record, adapter)),
            Err(error) => sandboxes.push(broken_record_entry(&entry.name, error)),
        }
    }
    Ok(json!({ "sandboxes": sandboxes }))
}

fn record_entry(root: &Path, record: &SandboxRecord, adapter: &dyn PlatformAdapter) -> Value {
    match status_reply(root, record, adapter) {
        Ok(reply) => success_value(&record.name, Some(&record.provider), reply),
        Err(error) => broken_entry(record, &error),
    }
}

fn status_reply(
    root: &Path,
    record: &SandboxRecord,
    adapter: &dyn PlatformAdapter,
) -> Result<ProviderReply, AppError> {
    let spec = recorded_provider(record)?;
    let (provider_dir, sandboxes_root) = resolve(root, spec)?;
    let call = ProviderCall::new(
        spec,
        &provider_dir,
        &sandboxes_root,
        Some(&record.name),
        &[],
    );
    let output = call.run(adapter, verb_args(&record.name, "status"), STATUS_TIMEOUT)?;
    parse_reply(spec.name, Some(&record.name), false, &output)
}

fn locate(root: &Path, name: &str) -> Result<(&'static ProviderSpec, PathBuf, PathBuf), AppError> {
    let record = sandbox_store::load(&root.join(SANDBOXES_DIR), name)
        .map_err(|error| recordless_directory(error, &root.join(SANDBOXES_DIR), name))?;
    let spec = recorded_provider(&record)?;
    let (provider_dir, sandboxes_root) = resolve(root, spec)?;
    Ok((spec, provider_dir, sandboxes_root))
}

/// Reports a sandbox directory that lost its record as broken, the way `sandbox list` shows it.
fn recordless_directory(error: AppError, sandboxes_root: &Path, name: &str) -> AppError {
    let directory_exists = std::fs::symlink_metadata(sandboxes_root.join(name))
        .is_ok_and(|metadata| metadata.is_dir());
    if directory_exists && error.code() == ErrorCode::SandboxNotFound.as_str() {
        sandbox_store::missing_record(name)
    } else {
        error
    }
}

fn recorded_provider(record: &SandboxRecord) -> Result<&'static ProviderSpec, AppError> {
    find_provider(&record.provider).map_err(|_| {
        AppError::Adapter(
            AdapterError::new(
                ErrorCode::InvalidArgs,
                format!(
                    "Sandbox {} records provider {}, which this build does not know.",
                    record.name, record.provider
                ),
            )
            .with_suggestion(unknown_provider_next(&record.name))
            .with_disposition(DeliverySemantics::not_delivered()),
        )
    })
}

fn unknown_provider_next(name: &str) -> String {
    format!(
        "Remove it with the agent-desktop build that created it. If the record is wrong, run `agent-desktop sandbox rm {name} --provider <provider>` with the provider that created it."
    )
}

fn verb_args(name: &str, verb: &str) -> Vec<String> {
    vec![verb.to_string(), name.to_string()]
}

fn is_mutating(verb: &str) -> bool {
    matches!(verb, "create" | "start" | "stop" | "rm")
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn running_refusal(name: &str) -> AppError {
    AppError::Adapter(
        AdapterError::new(
            ErrorCode::InvalidArgs,
            format!("Sandbox {name} is running, so it was not removed."),
        )
        .with_suggestion(format!("Run `agent-desktop sandbox stop {name}` first."))
        .with_disposition(DeliverySemantics::not_delivered()),
    )
}

fn broken_entry(record: &SandboxRecord, error: &AppError) -> Value {
    let next = if find_provider(&record.provider).is_ok() {
        format!(
            "Run `agent-desktop sandbox rm {}` to remove it.",
            record.name
        )
    } else {
        unknown_provider_next(&record.name)
    };
    broken_value(
        &record.name,
        Some(&record.provider),
        error.to_string(),
        next,
    )
}

fn broken_record_entry(name: &str, error: &AppError) -> Value {
    let (message, next) = if error.code() == ErrorCode::SandboxNotFound.as_str() {
        (
            sandbox_store::missing_record(name).to_string(),
            recovery_next(name),
        )
    } else if error.code() == ErrorCode::InvalidArgs.as_str() {
        (error.to_string(), recovery_next(name))
    } else {
        (
            format!("The record for sandbox {name} could not be read."),
            "Fix the record file's ownership and permissions, then run `agent-desktop sandbox list` again.".to_string(),
        )
    };
    broken_value(name, None, message, next)
}

fn broken_value(name: &str, provider: Option<&str>, message: String, next: String) -> Value {
    let mut value = json!({
        "name": name,
        "state": SandboxState::Broken,
        "message": message,
        "next": next,
    });
    if let Some(provider) = provider {
        value["provider"] = json!(provider);
    }
    value
}

fn recovery_next(name: &str) -> String {
    format!(
        "Run `agent-desktop sandbox rm {name} --provider <provider>` with the provider that created it."
    )
}

fn unavailable_refusal(message: String) -> AppError {
    AppError::Adapter(
        AdapterError::new(ErrorCode::PlatformNotSupported, message)
            .with_suggestion("Run `agent-desktop sandbox install` to see what each provider needs.")
            .with_disposition(DeliverySemantics::not_delivered()),
    )
}

fn exec_without_envelope(name: &str) -> AppError {
    AppError::Adapter(
        AdapterError::new(
            ErrorCode::ActionNotSupported,
            "`sandbox exec` prints the sandboxed program's own output, so it has no data envelope of its own.",
        )
        .with_suggestion(format!(
            "Run `agent-desktop sandbox exec {name} -- snapshot --app Finder` to run a command in the sandbox."
        ))
        .with_disposition(DeliverySemantics::not_delivered()),
    )
}

#[cfg(test)]
#[path = "sandbox_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "sandbox_recovery_tests.rs"]
mod recovery_tests;
