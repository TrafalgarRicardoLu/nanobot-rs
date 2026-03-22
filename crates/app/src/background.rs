use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nanobot_channels::ChannelRuntimeHandle;

use crate::{AppError, DispatchRecord, NanobotApp};

pub struct BackgroundWorkerHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<Vec<DispatchRecord>>>,
}

impl BackgroundWorkerHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn join(mut self) -> std::thread::Result<Vec<DispatchRecord>> {
        self.join.take().expect("join handle").join()
    }
}

impl NanobotApp {
    pub fn start_channel_runtimes(&self) -> Result<Vec<ChannelRuntimeHandle>, AppError> {
        let inbound_tx = self.bus.inbound_publisher();
        let mut handles = Vec::new();
        for channel in &self.channels {
            if let Some(handle) = channel.spawn_inbound_runtime(inbound_tx.clone()) {
                handles.push(handle);
            }
        }
        Ok(handles)
    }

    pub fn spawn_background_worker(
        app: Arc<Mutex<Self>>,
        start_tick: u64,
        tick_step: u64,
        interval_ms: u64,
        max_iterations: usize,
    ) -> BackgroundWorkerHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_worker = stop.clone();
        let join = thread::spawn(move || {
            let mut records = Vec::new();
            for index in 0..max_iterations {
                if stop_worker.load(Ordering::SeqCst) {
                    break;
                }
                let tick = start_tick + tick_step.saturating_mul(index as u64);
                let mut app = app.lock().expect("background app lock");
                if let Ok(mut batch) = app.pump_background_once(tick) {
                    records.append(&mut batch);
                }
                drop(app);
                if interval_ms > 0 {
                    thread::sleep(Duration::from_millis(interval_ms));
                }
            }
            records
        });
        BackgroundWorkerHandle {
            stop,
            join: Some(join),
        }
    }
}
