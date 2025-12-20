use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, Mac};
use rand::{Rng, RngCore, distributions::Alphanumeric, rngs::OsRng};
use reqwest::Certificate;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::Sha256;
use tracing::{debug, error, warn};

use crate::config;

const MAX_QUEUE_LEN: usize = 200;
const MAX_QUEUE_AGE: Duration = Duration::from_secs(60 * 30);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);
const BASE_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const SIGNING_KEY_FILE: &str = "telemetry-signing.key";
const PINNED_CERT_FILE: &str = "telemetry-cert.pem";

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueuedTelemetryEvent {
    event: TelemetryEvent,
    queued_at: i64,
}

pub struct TelemetryEmitter {
    enabled: bool,
    session: String,
    worker: TelemetryWorkerHandle,
}

impl Default for TelemetryEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetryEmitter {
    pub fn new() -> Self {
        Self::from_options(TelemetryOptions::production())
    }

    fn from_options(options: TelemetryOptions) -> Self {
        let session: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(24)
            .map(char::from)
            .collect();

        let emitter = Self {
            enabled: false,
            session,
            worker: TelemetryWorkerHandle::spawn(options),
        };
        emitter
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if let Err(_) = self.worker.send(WorkerCommand::SetEnabled(enabled)) {
            warn!(
                target: "gitspace::telemetry",
                "telemetry worker unavailable while updating enabled state"
            );
        }
    }

    pub fn record_event(&mut self, name: &str, properties: Map<String, Value>) {
        if !self.enabled {
            return;
        }

        let event = TelemetryEvent::new(name.to_string(), self.session.clone(), properties);
        let queued = QueuedTelemetryEvent {
            event,
            queued_at: Utc::now().timestamp(),
        };
        if let Err(_) = self.worker.send(WorkerCommand::Record(queued)) {
            warn!(
                target: "gitspace::telemetry",
                "telemetry worker unavailable; dropping event"
            );
        }
    }

    pub fn tick(&mut self) {
        if !self.enabled {
            return;
        }

        if let Err(_) = self.worker.send(WorkerCommand::Tick) {
            warn!(
                target: "gitspace::telemetry",
                "telemetry worker unavailable during tick"
            );
        }
    }

    pub fn purge(&mut self) {
        if let Err(_) = self.worker.send(WorkerCommand::Purge) {
            warn!(target: "gitspace::telemetry", "telemetry worker unavailable during purge");
        }
    }
}

#[derive(Clone)]
struct TelemetryOptions {
    offline_path: PathBuf,
    batch_size: usize,
    flush_interval: Duration,
    client: Client,
    endpoint: String,
    signing_key: Vec<u8>,
}

fn sign_payload(payload: &Value, key: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC can take key of any size");
    let serialized = serde_json::to_vec(payload).unwrap_or_default();
    mac.update(&serialized);
    let digest = mac.finalize().into_bytes();
    base64::engine::general_purpose::STANDARD.encode(digest)
}

fn build_client(pinned_cert: Option<Certificate>) -> Client {
    let mut builder = reqwest::blocking::ClientBuilder::new().timeout(CLIENT_TIMEOUT);
    if let Some(cert) = pinned_cert {
        builder = builder.add_root_certificate(cert);
    }
    builder
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

fn load_or_create_signing_key() -> Vec<u8> {
    let path = config::app_data_dir().join(SIGNING_KEY_FILE);
    if let Ok(bytes) = fs::read(&path) {
        if !bytes.is_empty() {
            return bytes;
        }
    }

    let mut key = vec![0u8; 32];
    OsRng.fill_bytes(&mut key);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(err) = fs::write(&path, &key) {
        warn!(target: "gitspace::telemetry", error = %err, "failed to persist telemetry signing key");
    }
    key
}

fn load_pinned_certificate() -> Option<Certificate> {
    let env_path = std::env::var("GITSPACE_TELEMETRY_CERT").ok();
    let config_path = config::app_data_dir().join(PINNED_CERT_FILE);
    let path = env_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| {
            if config_path.exists() {
                Some(config_path)
            } else {
                None
            }
        });

    if let Some(path) = path {
        match fs::read(&path) {
            Ok(bytes) => match Certificate::from_pem(&bytes) {
                Ok(cert) => Some(cert),
                Err(err) => {
                    warn!(
                        target: "gitspace::telemetry",
                        error = %err,
                        path = %path.display(),
                        "failed to load pinned certificate"
                    );
                    None
                }
            },
            Err(err) => {
                warn!(
                    target: "gitspace::telemetry",
                    error = %err,
                    path = %path.display(),
                    "failed to read pinned certificate"
                );
                None
            }
        }
    } else {
        None
    }
}

impl TelemetryOptions {
    fn production() -> Self {
        Self {
            offline_path: config::app_data_dir().join("telemetry-queue.json"),
            batch_size: 10,
            flush_interval: Duration::from_secs(30),
            client: build_client(load_pinned_certificate()),
            endpoint: "https://telemetry.gitspace.local/events".to_string(),
            signing_key: load_or_create_signing_key(),
        }
    }

    #[cfg(test)]
    fn for_tests(
        offline_path: PathBuf,
        client: Client,
        endpoint: String,
        flush_interval: Duration,
        batch_size: usize,
    ) -> Self {
        let mut signing_key = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut signing_key);
        Self {
            offline_path,
            batch_size,
            flush_interval,
            client,
            endpoint,
            signing_key,
        }
    }
}

#[derive(Debug)]
enum WorkerCommand {
    Record(QueuedTelemetryEvent),
    Tick,
    Purge,
    SetEnabled(bool),
}

#[derive(Clone)]
struct TelemetryWorkerHandle {
    sender: mpsc::Sender<WorkerCommand>,
}

impl TelemetryWorkerHandle {
    fn spawn(options: TelemetryOptions) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("telemetry-worker".to_string())
            .spawn(move || {
                TelemetryWorker::new(options).run(receiver);
            })
            .expect("telemetry worker thread");
        Self { sender }
    }

    fn send(&self, command: WorkerCommand) -> Result<(), mpsc::SendError<WorkerCommand>> {
        self.sender.send(command)
    }
}

struct TelemetryWorker {
    queue: VecDeque<QueuedTelemetryEvent>,
    offline_path: PathBuf,
    batch_size: usize,
    last_flush: Instant,
    flush_interval: Duration,
    client: Client,
    endpoint: String,
    next_retry_at: Option<Instant>,
    backoff_attempts: u32,
    signing_key: Vec<u8>,
    enabled: bool,
}

impl TelemetryWorker {
    fn new(options: TelemetryOptions) -> Self {
        let mut worker = Self {
            queue: VecDeque::new(),
            offline_path: options.offline_path,
            batch_size: options.batch_size,
            last_flush: Instant::now(),
            flush_interval: options.flush_interval,
            client: options.client,
            endpoint: options.endpoint,
            next_retry_at: None,
            backoff_attempts: 0,
            signing_key: options.signing_key,
            enabled: false,
        };
        worker.load_offline_queue();
        worker
    }

    fn run(mut self, receiver: mpsc::Receiver<WorkerCommand>) {
        for command in receiver {
            match command {
                WorkerCommand::Record(event) => self.record(event),
                WorkerCommand::Tick => self.tick(),
                WorkerCommand::Purge => self.purge(),
                WorkerCommand::SetEnabled(enabled) => self.set_enabled(enabled),
            }
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if enabled {
            self.load_offline_queue();
        } else {
            self.reset_state();
        }
    }

    fn reset_state(&mut self) {
        self.queue.clear();
        let _ = fs::remove_file(&self.offline_path);
        self.next_retry_at = None;
        self.backoff_attempts = 0;
    }

    fn record(&mut self, event: QueuedTelemetryEvent) {
        if !self.enabled {
            return;
        }
        self.queue.push_back(event);
        self.enforce_limits();
    }

    fn tick(&mut self) {
        if !self.enabled {
            return;
        }

        self.discard_stale_events();
        if let Some(next_retry) = self.next_retry_at {
            if Instant::now() < next_retry {
                return;
            }
        }

        let due_to_time = self.last_flush.elapsed() >= self.flush_interval;
        let due_to_size = self.queue.len() >= self.batch_size;
        if due_to_size || (due_to_time && !self.queue.is_empty()) {
            self.flush();
        }
    }

    fn purge(&mut self) {
        self.reset_state();
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

            let events: Vec<_> = batch.iter().map(|item| item.event.clone()).collect();
            let payload = json!({"events": events});
            let signature = sign_payload(&payload, &self.signing_key);
            let result = self
                .client
                .post(&self.endpoint)
                .header("X-GitSpace-Signature", signature)
                .json(&payload)
                .send()
                .and_then(|res| res.error_for_status());

            if let Err(err) = result {
                let mut drained = VecDeque::from(batch);
                drained.append(&mut self.queue);
                self.queue = drained;
                self.persist_offline();
                self.schedule_backoff();
                warn!(
                    target: "gitspace::telemetry",
                    path = %self.offline_path.display(),
                    "telemetry worker could not reach endpoint; using offline queue"
                );
                error!(target: "gitspace::telemetry", error = %err, "telemetry flush failed; queued events persisted locally");
                return;
            }

            self.last_flush = Instant::now();
            self.backoff_attempts = 0;
            self.next_retry_at = None;
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
            if let Ok(stored) = serde_json::from_str::<VecDeque<QueuedTelemetryEvent>>(&contents) {
                let loaded = stored.len();
                for event in stored {
                    self.queue.push_back(event);
                }
                if loaded > 0 {
                    warn!(
                        target: "gitspace::telemetry",
                        loaded,
                        path = %self.offline_path.display(),
                        "restored telemetry offline queue"
                    );
                }
            }
        }
        self.discard_stale_events();
        self.enforce_limits();
    }

    fn enforce_limits(&mut self) {
        while self.queue.len() > MAX_QUEUE_LEN {
            if let Some(evicted) = self.queue.pop_front() {
                warn!(
                    target: "gitspace::telemetry",
                    event = %evicted.event.name,
                    "telemetry queue full; dropping oldest"
                );
            }
        }
    }

    fn discard_stale_events(&mut self) {
        let cutoff = Utc::now().timestamp() - MAX_QUEUE_AGE.as_secs() as i64;
        let mut dropped = 0;
        self.queue.retain(|item| {
            let keep = item.queued_at >= cutoff;
            if !keep {
                dropped += 1;
            }
            keep
        });
        if dropped > 0 {
            warn!(target: "gitspace::telemetry", dropped, "removed expired telemetry events");
        }
    }

    fn schedule_backoff(&mut self) {
        self.backoff_attempts = self.backoff_attempts.saturating_add(1);
        let multiplier = 1u32 << self.backoff_attempts.min(5);
        let exponential = BASE_BACKOFF.checked_mul(multiplier).unwrap_or(MAX_BACKOFF);
        let delay = std::cmp::min(exponential, MAX_BACKOFF);
        let jitter_ms: u64 = rand::thread_rng().gen_range(0..=500);
        let delay = delay + Duration::from_millis(jitter_ms);
        self.next_retry_at = Some(Instant::now() + delay);
        debug!(target: "gitspace::telemetry", ?delay, "scheduled telemetry backoff");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{ErrorKind, Write};
    use std::net::TcpListener;
    use std::time::Instant;

    fn start_delayed_server(delay: Duration) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().unwrap();
        listener
            .set_nonblocking(true)
            .expect("nonblocking test server");
        let handle = thread::spawn(move || {
            let start = Instant::now();
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        thread::sleep(delay);
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
                        break;
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => break,
                }
                if start.elapsed() > Duration::from_secs(2) {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        (format!("http://{}", addr), handle)
    }

    fn short_timeout_client() -> Client {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(100))
            .build()
            .expect("short timeout client")
    }

    #[test]
    fn tick_returns_quickly_with_blocking_flush_in_worker() {
        let (endpoint, server_handle) = start_delayed_server(Duration::from_millis(150));
        let temp_dir = tempfile::tempdir().unwrap();
        let options = TelemetryOptions::for_tests(
            temp_dir.path().join("queue.json"),
            short_timeout_client(),
            endpoint,
            Duration::from_millis(5),
            1,
        );
        let mut emitter = TelemetryEmitter::from_options(options);
        emitter.set_enabled(true);
        emitter.record_event("test", Map::new());

        let started = Instant::now();
        emitter.tick();
        assert!(
            started.elapsed() < Duration::from_millis(30),
            "tick should return quickly"
        );

        drop(emitter);
        let _ = server_handle.join();
    }

    #[test]
    fn backoff_keeps_tick_responsive_and_persists_offline_queue() {
        let temp_dir = tempfile::tempdir().unwrap();
        let queue_path = temp_dir.path().join("queue.json");
        let options = TelemetryOptions::for_tests(
            queue_path.clone(),
            short_timeout_client(),
            "http://127.0.0.1:9".to_string(),
            Duration::from_millis(1),
            1,
        );
        let mut emitter = TelemetryEmitter::from_options(options);
        emitter.set_enabled(true);
        emitter.record_event("test", Map::new());

        emitter.tick();
        thread::sleep(Duration::from_millis(50));
        assert!(
            queue_path.exists(),
            "offline queue should be written after failed flush"
        );

        let start = Instant::now();
        emitter.tick();
        assert!(
            start.elapsed() < Duration::from_millis(30),
            "tick should stay responsive during backoff"
        );
    }
}
