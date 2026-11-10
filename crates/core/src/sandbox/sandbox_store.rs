use crate::sandbox::sandbox_entry::SandboxEntry;
use crate::sandbox::sandbox_name::validate_sandbox_name;
use crate::sandbox::sandbox_record::SandboxRecord;
use crate::storage::private_file::{read_private_bounded, write_atomic};
use crate::storage::private_file_parent::ensure_private;
use crate::{
    AdapterError, AppError, Deadline, DeliverySemantics, ErrorCode, storage::file_lock::FileLock,
};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub(crate) const RECORD_FILE: &str = "sandbox.json";
const CREATE_LOCK_FILE: &str = ".create.lock";
const MAX_RECORD_BYTES: u64 = 4096;

pub(crate) fn create(root: &Path, record: &SandboxRecord) -> Result<PathBuf, AppError> {
    validate_sandbox_name(&record.name)?;
    let dir = root.join(&record.name);
    ensure_private(&dir)?;
    let _lock = FileLock::acquire(
        &root.join(CREATE_LOCK_FILE),
        Deadline::standard()?,
        "sandbox record create",
    )?;
    let destination = dir.join(RECORD_FILE);
    if entry_exists(&destination)? {
        return Err(already_exists(&load(root, &record.name)?));
    }
    write_atomic(&destination, &serde_json::to_vec(record)?)?;
    Ok(dir)
}

fn entry_exists(path: &Path) -> Result<bool, AppError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn load(root: &Path, name: &str) -> Result<SandboxRecord, AppError> {
    validate_sandbox_name(name)?;
    match read_private_bounded(&root.join(name).join(RECORD_FILE), MAX_RECORD_BYTES) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .ok()
            .filter(|record: &SandboxRecord| record.name == name)
            .ok_or_else(|| invalid_record(name)),
        Err(error) if error.kind() == ErrorKind::NotFound => Err(not_found(name)),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn list(root: &Path) -> Result<Vec<SandboxEntry>, AppError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut sandboxes = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if validate_sandbox_name(&name).is_err() {
            continue;
        }
        let record = load(root, &name);
        sandboxes.push(SandboxEntry { name, record });
    }
    sandboxes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(sandboxes)
}

pub(crate) fn remove(root: &Path, name: &str) -> Result<(), AppError> {
    validate_sandbox_name(name)?;
    match std::fs::remove_dir_all(root.join(name)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn already_exists(record: &SandboxRecord) -> AppError {
    AppError::Adapter(
        AdapterError::new(
            ErrorCode::InvalidArgs,
            format!(
                "A sandbox named {} already exists (provider {}).",
                record.name, record.provider
            ),
        )
        .with_suggestion(format!(
            "Pick another name, or run `agent-desktop sandbox rm {}` first.",
            record.name
        ))
        .with_disposition(DeliverySemantics::not_delivered()),
    )
}

fn invalid_record(name: &str) -> AppError {
    AppError::Adapter(
        AdapterError::new(
            ErrorCode::InvalidArgs,
            format!(
                "The record for sandbox {name} is malformed, so agent-desktop cannot tell which provider owns it."
            ),
        )
        .with_suggestion(format!(
            "Run `agent-desktop sandbox rm {name} --provider <provider>` with the provider that created it."
        ))
        .with_disposition(DeliverySemantics::not_delivered()),
    )
}

pub(crate) fn missing_record(name: &str) -> AppError {
    AppError::Adapter(
        AdapterError::new(
            ErrorCode::InvalidArgs,
            format!("The sandbox directory {name} exists but its record file is missing."),
        )
        .with_suggestion(format!(
            "Run `agent-desktop sandbox rm {name} --provider <provider>` with the provider that created it."
        ))
        .with_disposition(DeliverySemantics::not_delivered()),
    )
}

pub(crate) fn not_found(name: &str) -> AppError {
    AppError::Adapter(
        AdapterError::new(
            ErrorCode::SandboxNotFound,
            format!("No sandbox named {name} exists."),
        )
        .with_suggestion("Run `agent-desktop sandbox list` to see the sandboxes that exist.")
        .with_disposition(DeliverySemantics::not_delivered()),
    )
}

#[cfg(test)]
#[path = "sandbox_store_tests.rs"]
mod tests;
