use agent_desktop_core::commands::batch::BatchCommand;

use super::parse_command;

fn item(command: &str) -> BatchCommand {
    BatchCommand {
        command: command.into(),
        session: None,
        args: serde_json::Value::Null,
    }
}

#[test]
fn sandbox_and_install_are_refused_inside_a_batch() {
    for command in ["sandbox", "install"] {
        let error = parse_command(item(command)).expect_err("batch must refuse the command");

        assert_eq!(error.code(), "INVALID_ARGS", "{command}");
    }
}
