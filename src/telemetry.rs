use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::Utc;
use rand::{Rng, distributions::Alphanumeric};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tracing::{debug, error};

use crate::config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub name: String,
    pub timestamp: String,
    pub session: String,
    pub properties: Map<String, Value>,
}

impl TelemetryEvent {
    pub fn new<T: Into<String>>(name: T, session: String, properties: Map<String, Value>) -> Self {
        Self {
            name: name.into(),
            timestamp: Utc::now().to_rfc3339(),
            session,
            properties,
        }
    }
}

pub struct TelemetryEmitter {
    enabled: bool,
    queue: VecDeque<TelemetryEvent>,
    offline_path: PathBuf,
    batch_size: usize,
    last_flush: Instant,
    flush_interval: Duration,
    client: Client,
    endpoint: String,
    session: String,
}

impl Default for TelemetryEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetryEmitter {
    pub fn new() -> Self {
        let offline_path = config::app_data_dir().join("telemetry-queue.json");
        let client = Client::new();
        let session: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(24)
            .map(char::from)
            .collect();

        let mut emitter = Self {
            enabled: false,
            queue: VecDeque::new(),
            offline_path,
            batch_size: 10,
            last_flush: Instant::now(),
            flush_interval: Duration::from_secs(30),
            client,
            endpoint: "https://telemetry.gitspace.local/events".to_string(),
            session,
        };
        emitter.load_offline_queue();
        emitter
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if enabled {
            self.load_offline_queue();
        } else {
            self.queue.clear();
            let _ = fs::remove_file(&self.offline_path);
        }
    }

    pub fn record_event(&mut self, name: &str, properties: Map<String, Value>) {
        if !self.enabled {
            return;
        }

        let event = TelemetryEvent::new(name.to_string(), self.session.clone(), properties);
        self.queue.push_back(event);
    }

    pub fn tick(&mut self) {
        if !self.enabled {
            return;
        }

        let due_to_time = self.last_flush.elapsed() >= self.flush_interval;
        let due_to_size = self.queue.len() >= self.batch_size;
        if due_to_size || (due_to_time && !self.queue.is_empty()) {
            self.flush();
        }
    }

    pub fn purge(&mut self) {
        self.queue.clear();
        let _ = fs::remove_file(&self.offline_path);
    }

    fn flush(&mut self) {
        while !self.queue.is_empty() {
            let mut batch = Vec::new();
            for _ in 0..self.batch_size {
                if let Some(item) = self.queue.pop_front() {
                    batch.push(item);
                }
            }

            if batch.is_empty() {
                break;
            }

            let payload = json!({"events": batch});
            let result = self
                .client
                .post(&self.endpoint)
                .json(&payload)
                .send()
                .and_then(|res| res.error_for_status());

            if let Err(err) = result {
                let mut drained = VecDeque::from(batch);
                drained.append(&mut self.queue);
                self.queue = drained;
                self.persist_offline();
                error!(target: "gitspace::telemetry", error = %err, "telemetry flush failed; queued events persisted locally");
                return;
            }

            self.last_flush = Instant::now();
            debug!(
                target: "gitspace::telemetry",
                queued = self.queue.len(),
                "telemetry batch flushed"
            );
        }

        if self.queue.is_empty() {
            let _ = fs::remove_file(&self.offline_path);
        }
    }

    fn persist_offline(&self) {
        if let Some(parent) = self.offline_path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                error!(target: "gitspace::telemetry", error = %err, "telemetry queue dir error");
                return;
            }
        }

        if let Err(err) = fs::write(
            &self.offline_path,
            serde_json::to_string(&self.queue).unwrap_or_default(),
        ) {
            error!(target: "gitspace::telemetry", error = %err, "telemetry queue write error");
        }
    }

    fn load_offline_queue(&mut self) {
        if let Ok(contents) = fs::read_to_string(&self.offline_path) {
            if let Ok(stored) = serde_json::from_str::<VecDeque<TelemetryEvent>>(&contents) {
                for event in stored {
                    self.queue.push_back(event);
                }
            }
        }
    }
}
