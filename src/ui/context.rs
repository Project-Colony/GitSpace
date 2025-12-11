use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoContext {
    pub path: String,
    pub name: String,
}

impl RepoContext {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Self {
        let path_ref = path.as_ref();
        let path_string = path_ref.to_string_lossy().to_string();
        let name = path_ref
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&path_string)
            .to_string();

        Self {
            path: path_string,
            name,
        }
    }
}
