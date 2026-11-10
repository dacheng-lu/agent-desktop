use agent_desktop_core::commands::sandbox_action::SandboxAction;
use agent_desktop_core::{AppError, ErrorCode};
use clap::Parser;

use super::{reject_outer_options, to_core};

#[test]
fn outer_global_options_are_rejected_before_sandbox_exec() {
    let plain =
        crate::Cli::try_parse_from(["agent-desktop", "sandbox", "exec", "dev", "--", "version"])
            .unwrap();
    assert!(reject_outer_options(&plain).is_ok());

    let outer = crate::Cli::try_parse_from([
        "agent-desktop",
        "--session",
        "abc",
        "sandbox",
        "exec",
        "dev",
        "--",
        "version",
    ])
    .unwrap();
    let AppError::Adapter(error) = reject_outer_options(&outer).unwrap_err() else {
        panic!("outer options must be an adapter error");
    };

    assert_eq!(error.code, ErrorCode::InvalidArgs);
    assert!(
        error.suggestion.is_some(),
        "the refusal must say where the options go"
    );
}

struct IdleAdapter;
impl agent_desktop_core::ObservationOps for IdleAdapter {}
impl agent_desktop_core::ActionOps for IdleAdapter {}
impl agent_desktop_core::InputOps for IdleAdapter {}
impl agent_desktop_core::SystemOps for IdleAdapter {}

#[test]
fn a_sandbox_exec_refusal_reports_that_nothing_was_delivered() {
    let cli = crate::Cli::try_parse_from(["agent-desktop", "sandbox", "exec", "BAD", "--", "ls"])
        .unwrap();
    let Some(crate::cli::Commands::Sandbox(args)) = &cli.command else {
        panic!("sandbox command");
    };
    let crate::cli_args::sandbox::SandboxActionCli::Exec(exec) = &args.action else {
        panic!("exec action");
    };

    let AppError::Adapter(error) = super::exec_passthrough(&cli, exec, &IdleAdapter).unwrap_err()
    else {
        panic!("an invalid name must be an adapter error");
    };

    assert_eq!(error.code, ErrorCode::InvalidArgs);
    assert_eq!(
        error.disposition,
        agent_desktop_core::DeliverySemantics::not_delivered()
    );
}

#[test]
fn sandbox_rm_forwards_the_recovery_provider_to_core() {
    let cli = crate::Cli::try_parse_from([
        "agent-desktop",
        "sandbox",
        "rm",
        "dev-1",
        "--provider",
        "macos",
    ])
    .unwrap();
    let Some(crate::cli::Commands::Sandbox(args)) = cli.command else {
        panic!("sandbox command");
    };

    let SandboxAction::Rm { name, provider } = to_core(args.action) else {
        panic!("rm action");
    };

    assert_eq!(name, "dev-1");
    assert_eq!(provider.as_deref(), Some("macos"));
}
