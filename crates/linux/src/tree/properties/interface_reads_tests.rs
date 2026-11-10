use super::{ACTION_INTERFACE, skipped_without};
use crate::tree::Field;
use crate::tree::properties::liveness::Liveness;
use crate::tree::properties::node_read::COMPONENT_INTERFACE;

#[test]
fn a_hung_get_actions_is_a_failed_field_not_a_walk_timeout() {
    use std::time::{Duration, Instant};

    use zbus::connection::Builder;

    struct SlowAction;

    #[zbus::interface(name = "org.a11y.atspi.Action")]
    impl SlowAction {
        async fn get_actions(&self) -> Vec<(String, String, String)> {
            async_io::Timer::after(Duration::from_secs(5)).await;
            Vec::new()
        }
    }

    let (near, far) = std::os::unix::net::UnixStream::pair().unwrap();
    let (client, server) = zbus::block_on(futures_lite::future::zip(
        Builder::async_io_unix_stream(near).p2p().build(),
        Builder::async_io_unix_stream(far)
            .server(zbus::Guid::generate())
            .unwrap()
            .p2p()
            .serve_at("/node", SlowAction)
            .unwrap()
            .build(),
    ));
    let _server = server.unwrap();
    let element = crate::tree::AtspiElement::new(":1.1", "/node", 1);
    let interfaces = Field::Present(vec![ACTION_INTERFACE.to_owned()]);
    let started = Instant::now();
    let actions = zbus::block_on(super::read_actions(
        &client.unwrap(),
        &element,
        &interfaces,
        Liveness::Alive,
    ))
    .expect("a slow action list keeps the node readable");
    assert!(matches!(
        actions,
        Field::Failed(ref detail) if detail == "GetActions did not answer in time"
    ));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the action read outlived its slice"
    );
}

#[test]
fn action_name_transport_loss_cannot_use_the_listed_fallback() {
    let connection = crate::tree::live::test_support::closed_connection();
    let element = crate::tree::AtspiElement::new(":1.1", "/button", 1);
    let error = zbus::block_on(super::action_name(
        &connection,
        &element,
        0,
        "click".to_owned(),
    ))
    .expect_err("transport loss must end observation");
    assert_eq!(error.code, agent_desktop_core::ErrorCode::Internal);
}

fn listed(names: &[&str]) -> Field<Vec<String>> {
    Field::Present(names.iter().map(|name| (*name).to_owned()).collect())
}

#[test]
fn an_advertised_interface_is_read() {
    let interfaces = listed(&[COMPONENT_INTERFACE]);
    assert!(skipped_without::<()>(&interfaces, COMPONENT_INTERFACE).is_none());
}

#[test]
fn an_unadvertised_interface_is_absent() {
    let interfaces = listed(&[COMPONENT_INTERFACE]);
    assert_eq!(
        skipped_without::<()>(&interfaces, ACTION_INTERFACE),
        Some(Field::Absent)
    );
}

#[test]
fn an_unreadable_interface_list_makes_the_dependent_field_a_failure() {
    let interfaces = Field::Failed("timed out".to_owned());
    let skipped = skipped_without::<()>(&interfaces, ACTION_INTERFACE);
    assert!(skipped.is_some_and(|field| field.is_failed()));
}
