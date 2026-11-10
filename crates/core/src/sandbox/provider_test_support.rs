//! A scripted adapter double: it records every provider request and replays a canned reply per verb.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::adapter::{ActionOps, InputOps, ObservationOps, SystemOps};
use crate::{AdapterError, ErrorCode, ProviderScriptOutput, ProviderScriptRequest};

#[derive(Default)]
pub(crate) struct ScriptedAdapter {
    requests: Mutex<Vec<ProviderScriptRequest>>,
    replies: Mutex<HashMap<String, Result<ProviderScriptOutput, AdapterError>>>,
}

impl ScriptedAdapter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn reply(self, verb: &str, stdout: &str, status: i32) -> Self {
        self.insert(verb, Ok(output(stdout, status)));
        self
    }

    pub(crate) fn fail(self, verb: &str, error: AdapterError) -> Self {
        self.insert(verb, Err(error));
        self
    }

    fn insert(&self, verb: &str, reply: Result<ProviderScriptOutput, AdapterError>) {
        if let Ok(mut replies) = self.replies.lock() {
            replies.insert(verb.to_string(), reply);
        }
    }

    pub(crate) fn requests(&self) -> Vec<ProviderScriptRequest> {
        self.requests
            .lock()
            .map(|requests| requests.clone())
            .unwrap_or_default()
    }
}

fn output(stdout: &str, status: i32) -> ProviderScriptOutput {
    ProviderScriptOutput {
        status: Some(status),
        stdout: stdout.as_bytes().to_vec(),
        stderr_tail: String::new(),
    }
}

impl ObservationOps for ScriptedAdapter {}
impl ActionOps for ScriptedAdapter {}
impl InputOps for ScriptedAdapter {}

impl SystemOps for ScriptedAdapter {
    fn run_provider_script(
        &self,
        request: &ProviderScriptRequest,
    ) -> Result<ProviderScriptOutput, AdapterError> {
        if let Ok(mut requests) = self.requests.lock() {
            requests.push(request.clone());
        }
        let verb = request.args.first().cloned().unwrap_or_default();
        self.replies
            .lock()
            .ok()
            .and_then(|replies| replies.get(&verb).cloned())
            .unwrap_or_else(|| {
                Err(AdapterError::new(
                    ErrorCode::ActionFailed,
                    format!("The stub adapter has no reply for {verb}."),
                )
                .with_disposition(crate::DeliverySemantics::not_delivered()))
            })
    }
}
