use std::path::{Path, PathBuf};

use super::FetchResult;
use super::error::FetchError;

pub(super) struct FileLocator {
    base_dir: Option<PathBuf>,
}

impl FileLocator {
    pub(super) fn new() -> Self {
        Self { base_dir: None }
    }

    pub(super) fn with_base_dir(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: Some(base_dir.as_ref().to_path_buf()),
        }
    }

    pub(super) fn set_base_dir(&mut self, base_dir: impl AsRef<Path>) {
        self.base_dir = Some(base_dir.as_ref().to_path_buf());
    }

    pub(super) fn direct_path(url: &str) -> Option<PathBuf> {
        if let Some(path) = url.strip_prefix("file://") {
            return Some(PathBuf::from(path));
        }
        let path = Path::new(url);
        if path.is_absolute() {
            return Some(path.to_path_buf());
        }
        None
    }

    pub(super) fn relative_candidates(&self, url: &str) -> Vec<PathBuf> {
        let path = Path::new(url);
        self.base_dir
            .iter()
            .map(|base| base.join(path))
            .chain([path.to_path_buf()])
            .collect()
    }
}

pub(super) fn request_error(url: &str, message: impl ToString) -> crate::error::Error {
    FetchError::RequestFailed {
        url: url.to_string(),
        message: message.to_string(),
    }
    .into()
}

pub(super) fn fetched_file(path: &Path, content: Vec<u8>) -> FetchResult {
    FetchResult {
        content,
        final_url: format!("file://{}", path.display()),
        redirected: false,
    }
}

macro_rules! impl_file_fetcher_api {
    ($name:ident) => {
        impl $name {
            /// Creates a file fetcher without a base directory.
            pub fn new() -> Self {
                Self {
                    locator: FileLocator::new(),
                }
            }

            /// Creates a file fetcher with a base directory.
            pub fn with_base_dir(base_dir: impl AsRef<Path>) -> Self {
                Self {
                    locator: FileLocator::with_base_dir(base_dir),
                }
            }

            /// Sets the base directory for relative paths.
            pub fn base_dir(mut self, base_dir: impl AsRef<Path>) -> Self {
                self.locator.set_base_dir(base_dir);
                self
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

pub(super) use impl_file_fetcher_api;
