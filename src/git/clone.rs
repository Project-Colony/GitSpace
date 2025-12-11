use git2::{Cred, FetchOptions, RemoteCallbacks, build::RepoBuilder};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CloneRequest {
    pub url: String,
    pub destination: PathBuf,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CloneProgress {
    pub received_objects: usize,
    pub total_objects: usize,
    pub indexed_objects: usize,
    pub total_deltas: usize,
    pub indexed_deltas: usize,
    pub received_bytes: usize,
}

pub fn clone_repository(
    request: CloneRequest,
    mut on_progress: impl FnMut(CloneProgress) + Send + 'static,
) -> Result<(), String> {
    let mut callbacks = RemoteCallbacks::new();
    let token = request.token.clone();

    callbacks.credentials(move |_url, username_from_url, _allowed| {
        if let Some(token) = token.clone() {
            let username = username_from_url.unwrap_or("git");
            Cred::userpass_plaintext(username, &token)
        } else {
            Cred::default()
        }
    });

    callbacks.transfer_progress(move |stats| {
        on_progress(CloneProgress {
            received_objects: stats.received_objects(),
            total_objects: stats.total_objects(),
            indexed_objects: stats.indexed_objects(),
            total_deltas: stats.total_deltas(),
            indexed_deltas: stats.indexed_deltas(),
            received_bytes: stats.received_bytes(),
        });
        true
    });

    let mut fetch = FetchOptions::new();
    fetch.remote_callbacks(callbacks);

    let mut builder = RepoBuilder::new();
    builder.fetch_options(fetch);

    builder
        .clone(&request.url, &request.destination)
        .map_err(|e| e.message().to_string())?;

    Ok(())
}
