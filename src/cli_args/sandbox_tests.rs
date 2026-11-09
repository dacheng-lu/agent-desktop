use clap::Parser;

use super::SandboxActionCli;
use crate::cli::{Cli, Commands};

fn exec_action(argv: &[&str]) -> super::SandboxExecArgs {
    let mut full = vec!["agent-desktop", "sandbox"];
    full.extend_from_slice(argv);
    let cli = Cli::try_parse_from(full).expect("the sandbox command parses");
    let Commands::Sandbox(args) = cli.command.expect("the sandbox command is present") else {
        panic!("expected the sandbox command");
    };
    let SandboxActionCli::Exec(exec) = args.action else {
        panic!("expected the exec action");
    };
    exec
}

fn rm_action(argv: &[&str]) -> super::SandboxRmArgs {
    let mut full = vec!["agent-desktop", "sandbox", "rm"];
    full.extend_from_slice(argv);
    let cli = Cli::try_parse_from(full).expect("the sandbox rm command parses");
    let Commands::Sandbox(args) = cli.command.expect("the sandbox command is present") else {
        panic!("expected the sandbox command");
    };
    let SandboxActionCli::Rm(rm) = args.action else {
        panic!("expected the rm action");
    };
    rm
}

#[test]
fn sandbox_rm_accepts_an_optional_recovery_provider() {
    let plain = rm_action(&["dev-1"]);
    assert_eq!(plain.name, "dev-1");
    assert!(plain.provider.is_none());

    let recovery = rm_action(&["dev-1", "--provider", "macos-vm"]);
    assert_eq!(recovery.name, "dev-1");
    assert_eq!(recovery.provider.as_deref(), Some("macos-vm"));
}

#[test]
fn sandbox_exec_hands_every_argument_after_the_separator_to_the_guest() {
    let exec = exec_action(&[
        "exec",
        "dev",
        "--",
        "snapshot",
        "--app",
        "Finder",
        "--verbose",
    ]);

    assert_eq!(exec.name, "dev");
    assert_eq!(exec.args, ["snapshot", "--app", "Finder", "--verbose"]);
}
