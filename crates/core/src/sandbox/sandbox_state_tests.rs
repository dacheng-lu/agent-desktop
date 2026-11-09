use super::SandboxState;

#[test]
fn every_documented_state_name_is_accepted() {
    let documented = [
        ("unavailable", SandboxState::Unavailable),
        ("missing", SandboxState::Missing),
        ("ready", SandboxState::Ready),
        ("working", SandboxState::Working),
        ("needs_setup", SandboxState::NeedsSetup),
        ("running", SandboxState::Running),
        ("stopped", SandboxState::Stopped),
        ("broken", SandboxState::Broken),
    ];

    for (name, state) in documented {
        assert_eq!(SandboxState::from_wire(name), Some(state), "{name}");
    }
}
