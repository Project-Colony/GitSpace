use std::path::Path;

use git2::Repository;

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
}

pub fn list_remotes<P: AsRef<Path>>(path: P) -> Result<Vec<RemoteInfo>, git2::Error> {
    let repo = Repository::open(path)?;
    let mut remotes = Vec::new();

    if let Ok(names) = repo.remotes() {
        for name in names.iter().flatten() {
            if let Ok(remote) = repo.find_remote(name) {
                let url = remote.url().unwrap_or("(no url)").to_string();
                remotes.push(RemoteInfo {
                    name: name.to_string(),
                    url,
                });
            }
        }
    }

    Ok(remotes)
}
