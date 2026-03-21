use super::HeartbeatService;

#[test]
fn heartbeat_becomes_due_after_interval() {
    let mut heartbeat = HeartbeatService::new(3);
    assert!(!heartbeat.is_due(2));
    assert!(heartbeat.is_due(3));
    heartbeat.mark_sent(3);
    assert!(!heartbeat.is_due(5));
    assert!(heartbeat.is_due(6));
}
