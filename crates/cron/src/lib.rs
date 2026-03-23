use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CronJob {
    pub name: String,
    pub session_id: String,
    pub payload: String,
    pub interval_ticks: u64,
    pub next_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CronError {
    #[error("job name is blocked: {0}")]
    BlockedJobName(String),
    #[error("job name already exists: {0}")]
    DuplicateJobName(String),
}

#[derive(Debug, Default, Clone)]
pub struct CronService {
    jobs: HashMap<String, CronJob>,
    blocked_names: HashSet<String>,
}

impl CronService {
    pub fn add_job(
        &mut self,
        name: impl Into<String>,
        session_id: impl Into<String>,
        payload: impl Into<String>,
        interval_ticks: u64,
        next_tick: u64,
    ) -> Result<(), CronError> {
        let name = name.into();
        if self.blocked_names.contains(&name) {
            return Err(CronError::BlockedJobName(name));
        }
        if self.jobs.contains_key(&name) {
            return Err(CronError::DuplicateJobName(name));
        }
        self.jobs.insert(
            name.clone(),
            CronJob {
                name,
                session_id: session_id.into(),
                payload: payload.into(),
                interval_ticks: interval_ticks.max(1),
                next_tick,
            },
        );
        Ok(())
    }

    pub fn block_job_name(&mut self, name: impl Into<String>) {
        self.blocked_names.insert(name.into());
    }

    pub fn tick(&mut self, now_tick: u64) -> Vec<CronJob> {
        let mut due = Vec::new();
        for job in self.jobs.values_mut() {
            if now_tick >= job.next_tick {
                due.push(CronJob {
                    name: job.name.clone(),
                    session_id: job.session_id.clone(),
                    payload: job.payload.clone(),
                    interval_ticks: job.interval_ticks,
                    next_tick: job.next_tick,
                });
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
mod tests;
