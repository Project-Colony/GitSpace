use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

const DEFAULT_LOG_FILTER: &str = "gitspace=info,info";
const LOG_ENV: &str = "GITSPACE_LOG";

pub fn init_tracing() {
    if tracing::dispatcher::has_been_set() {
        return;
    }

    let env_filter = std::env::var(LOG_ENV)
        .ok()
        .and_then(|value| EnvFilter::try_new(value).ok())
        .unwrap_or_else(|| EnvFilter::new(DEFAULT_LOG_FILTER));

    let fmt_layer = fmt::layer()
        .with_thread_ids(false)
        .with_file(true)
        .with_line_number(true)
        .json();

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}
