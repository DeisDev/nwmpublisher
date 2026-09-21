use std::{
	io,
	path::{Path, PathBuf},
	sync::Arc,
};

#[derive(Debug, Clone, thiserror::Error)]
#[error("{operation} \"{}\": {source}", path.display())]
pub struct IoError {
	pub operation: &'static str,
	pub path: PathBuf,
	pub source: Arc<io::Error>,
}

impl IoError {
	pub fn new(operation: &'static str, path: &Path, source: io::Error) -> Self {
		Self {
			operation,
			path: path.to_owned(),
			source: Arc::new(source),
		}
	}
}
