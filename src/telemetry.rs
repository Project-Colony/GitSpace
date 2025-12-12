use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
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
    queue: VecDeque<QueuedTelemetryEvent>,
    offline_path: PathBuf,
    batch_size: usize,
    last_flush: Instant,
    flush_interval: Duration,
    client: Client,
    endpoint: String,
    session: String,
    next_retry_at: Option<Instant>,
    backoff_attempts: u32,
    signing_key: Vec<u8>,
}

impl Default for TelemetryEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetryEmitter {
    pub fn new() -> Self {
        let offline_path = config::app_data_dir().join("telemetry-queue.json");
        let client = build_client(load_pinned_certificate());
        let signing_key = load_or_create_signing_key();
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
            next_retry_at: None,
            backoff_attempts: 0,
            signing_key,
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
            self.next_retry_at = None;
            self.backoff_attempts = 0;
        }
    }

    pub fn record_event(&mut self, name: &str, properties: Map<String, Value>) {
        if !self.enabled {
            return;
        }

        let event = TelemetryEvent::new(name.to_string(), self.session.clone(), properties);
        self.queue.push_back(QueuedTelemetryEvent {
            event,
            queued_at: Utc::now().timestamp(),
        });
        self.enforce_limits();
    }

    pub fn tick(&mut self) {
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
                for event in stored {
                    self.queue.push_back(event);
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
