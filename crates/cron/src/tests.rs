use super::{CronError, CronJob, CronService};

#[test]
fn due_jobs_run_only_once_per_tick() {
    let mut cron = CronService::default();
    cron.add_job("digest", "cron:digest:session-1", "send-digest", 5, 10)
        .expect("job should be registered");

    assert_eq!(
        cron.tick(10),
        vec![CronJob {
            name: "digest".to_string(),
            session_id: "cron:digest:session-1".to_string(),
            payload: "send-digest".to_string(),
            interval_ticks: 5,
            next_tick: 10,
        }]
    );
    assert!(cron.tick(10).is_empty());
    assert!(cron.tick(14).is_empty());
    assert_eq!(
        cron.tick(15),
        vec![CronJob {
            name: "digest".to_string(),
            session_id: "cron:digest:session-1".to_string(),
            payload: "send-digest".to_string(),
            interval_ticks: 5,
            next_tick: 15,
        }]
    );
}

#[test]
fn cron_guard_blocks_self_scheduling_job_names() {
    let mut cron = CronService::default();
    cron.block_job_name("self-job");

    assert_eq!(
        cron.add_job("self-job", "cron:self-job:1", "blocked", 1, 1),
        Err(CronError::BlockedJobName("self-job".to_string()))
    );
    cron.add_job("other-job", "cron:other-job:1", "allowed", 1, 1)
        .expect("non-blocked job should register");
}

#[test]
fn cron_rejects_duplicate_job_names() {
    let mut cron = CronService::default();
    cron.add_job("digest", "cron:digest:session-1", "send-digest", 5, 10)
        .expect("first job should register");

    assert_eq!(
        cron.add_job(
            "digest",
            "cron:digest:session-2",
            "send-digest-again",
            10,
            20
        ),
        Err(CronError::DuplicateJobName("digest".to_string()))
    );
}
