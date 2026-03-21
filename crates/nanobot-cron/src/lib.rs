use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CronJob {
    pub name: String,
    pub interval_ticks: u64,
    pub next_tick: u64,
}

#[derive(Debug, Default, Clone)]
pub struct CronService {
    jobs: Vec<CronJob>,
    blocked_names: HashSet<String>,
}

impl CronService {
    pub fn add_job(
        &mut self,
        name: impl Into<String>,
        interval_ticks: u64,
        next_tick: u64,
    ) -> bool {
        let name = name.into();
        if self.blocked_names.contains(&name) {
            return false;
        }
        self.jobs.push(CronJob {
            name,
            interval_ticks: interval_ticks.max(1),
            next_tick,
        });
        true
    }

    pub fn block_job_name(&mut self, name: impl Into<String>) {
        self.blocked_names.insert(name.into());
    }

    pub fn tick(&mut self, now_tick: u64) -> Vec<String> {
        let mut due = Vec::new();
        for job in &mut self.jobs {
            if now_tick >= job.next_tick {
                due.push(job.name.clone());
                job.next_tick = now_tick + job.interval_ticks;
            }
        }
        due
    }

    pub fn job_count(&self) -> usize {
        self.jobs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::CronService;

    #[test]
    fn due_jobs_run_only_once_per_tick() {
        let mut cron = CronService::default();
        cron.add_job("digest", 5, 10);

        assert_eq!(cron.tick(10), vec!["digest".to_string()]);
        assert!(cron.tick(10).is_empty());
        assert!(cron.tick(14).is_empty());
        assert_eq!(cron.tick(15), vec!["digest".to_string()]);
    }

    #[test]
    fn cron_guard_blocks_self_scheduling_job_names() {
        let mut cron = CronService::default();
        cron.block_job_name("self-job");

        assert!(!cron.add_job("self-job", 1, 1));
        assert!(cron.add_job("other-job", 1, 1));
    }
}
