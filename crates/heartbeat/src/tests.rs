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

#[test]
fn heartbeat_tick_marks_sent_when_due() {
    let mut heartbeat = HeartbeatService::new(3);

    assert_eq!(heartbeat.tick(2), None);
    assert_eq!(heartbeat.tick(3), Some(3));
    assert_eq!(heartbeat.tick(5), None);
    assert_eq!(heartbeat.tick(6), Some(6));
}
