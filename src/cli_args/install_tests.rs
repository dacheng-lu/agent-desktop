use clap::Parser;

use crate::cli::Cli;

#[test]
fn install_no_longer_accepts_a_set_option() {
    let error = Cli::try_parse_from([
        "agent-desktop",
        "install",
        "macos-vm",
        "--set",
        "binary=/tmp/agent-desktop",
    ])
    .expect_err("install takes no provider settings");

    assert_eq!(error.exit_code(), 2);
}
