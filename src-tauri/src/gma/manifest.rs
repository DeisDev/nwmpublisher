use std::{
	collections::HashSet,
	path::{Path, PathBuf},
};

use path_slash::PathExt;
use walkdir::WalkDir;

use super::{whitelist, GMAEntry, GMAError};

#[derive(Debug)]
pub struct ContentManifest {
	entries: Vec<ContentEntry>,
}

#[derive(Debug)]
pub(super) struct ContentEntry {
	pub source_path: PathBuf,
	pub archive_path: String,
	pub size: u64,
}

impl ContentManifest {
	pub fn build(root: &Path, ignore: &[String], mut is_cancelled: impl FnMut() -> bool) -> Result<Self, GMAError> {
		if is_cancelled() {
			return Err(GMAError::Cancelled);
		}
		if !root.is_absolute()
			|| !root
				.metadata()
				.map_err(|error| GMAError::io("read content metadata", root, error))?
				.is_dir()
		{
			return Err(GMAError::InvalidContentPath);
		}
		Self::collect(root, ignore, is_cancelled, WalkDir::new(root).follow_links(false))
	}

	fn collect(
		root: &Path,
		ignore: &[String],
		mut is_cancelled: impl FnMut() -> bool,
		walk: impl IntoIterator<Item = Result<walkdir::DirEntry, walkdir::Error>>,
	) -> Result<Self, GMAError> {
		let mut entries = Vec::new();
		let mut paths = HashSet::new();
		let mut failed = Vec::new();
		let mut failed_extra = false;
		for entry in walk {
			if is_cancelled() {
				return Err(GMAError::Cancelled);
			}
			let entry = entry.map_err(|error| {
				let path = error.path().unwrap_or(root).to_owned();
				GMAError::io("read content directory", &path, error.into())
			})?;
			if !entry.file_type().is_file() {
				continue;
			}
			let source_path = entry.into_path();
			let archive_path = source_path
				.strip_prefix(root)
				.map_err(|_| GMAError::InvalidContentPath)?
				.to_slash_lossy()
				.to_lowercase();
			if !whitelist::filter_default_ignored(&archive_path) || whitelist::is_ignored(&archive_path, ignore) {
				continue;
			}
			if !paths.insert(archive_path.clone()) {
				return Err(GMAError::DuplicateEntry(archive_path));
			}
			if !whitelist::check(&archive_path) {
				if failed.len() < 9 {
					failed.push(archive_path);
				} else {
					failed_extra = true;
				}
				// Keep traversing so the diagnostic limit cannot hide directory errors.
				continue;
			}
			let size = source_path
				.metadata()
				.map_err(|error| GMAError::io("read source metadata", &source_path, error))?
				.len();
			entries.push(ContentEntry {
				source_path,
				archive_path,
				size,
			});
		}
		if is_cancelled() {
			return Err(GMAError::Cancelled);
		}
		if !failed.is_empty() {
			failed.sort_unstable();
			if failed_extra {
				failed.push("...".to_owned());
			}
			return Err(GMAError::NotWhitelisted(failed));
		}
		if entries.is_empty() {
			return Err(GMAError::NoEntries);
		}
		entries.sort_unstable_by(|a, b| a.archive_path.cmp(&b.archive_path));
		Ok(Self { entries })
	}

	pub fn into_preview(self) -> (Vec<GMAEntry>, u64) {
		let size = self.entries.iter().map(|entry| entry.size).sum();
		let entries = self
			.entries
			.into_iter()
			.map(|entry| GMAEntry {
				path: entry.archive_path,
				size: entry.size,
				crc: 0,
				index: 0,
			})
			.collect();
		(entries, size)
	}

	pub(super) fn into_entries(self) -> Vec<ContentEntry> {
		self.entries
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;

	#[test]
	fn normalizes_paths_and_applies_exclusions_before_the_whitelist() {
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().join("content-É");
		fs::create_dir_all(root.join("LUA/Nested")).unwrap();
		fs::write(root.join("LUA/Nested/Test.LUA"), b"test").unwrap();
		fs::write(root.join("LUA/skip.LUA"), b"ignored").unwrap();
		fs::write(root.join("ignored.exe"), b"ignored").unwrap();
		for path in ["README.md", ".DS_Store", "addon.json"] {
			fs::write(root.join(path), b"default ignored").unwrap();
		}
		let ignore = vec!["lua/skip.lua".to_owned(), "*.exe".to_owned()];
		let manifest = ContentManifest::build(&root.join(""), &ignore, || false).unwrap();
		assert_eq!(manifest.entries[0].source_path, root.join("LUA/Nested/Test.LUA"));
		let (preview, size) = manifest.into_preview();
		assert_eq!(preview.len(), 1);
		assert_eq!(preview[0].path, "lua/nested/test.lua");
		assert_eq!(preview[0].size, 4);
		assert_eq!(size, 4);
	}

	#[test]
	fn rejects_duplicate_normalized_paths_on_all_platforms() {
		let directory = tempfile::tempdir().unwrap();
		fs::create_dir(directory.path().join("lua")).unwrap();
		// These distinct filenames lowercase identically even on case-insensitive Windows volumes.
		fs::write(directory.path().join("lua/İ.lua"), b"first").unwrap();
		fs::write(directory.path().join("lua/i\u{307}.lua"), b"second").unwrap();
		let error = ContentManifest::build(directory.path(), &[], || false).unwrap_err();
		assert!(matches!(error, GMAError::DuplicateEntry(ref path) if path == "lua/i\u{307}.lua"));
		assert_eq!(serde_json::to_value(&error).unwrap(), error.to_string());
	}

	#[cfg(unix)]
	#[test]
	fn rejects_ascii_case_collisions_and_skips_symlinks() {
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path();
		fs::create_dir(root.join("lua")).unwrap();
		fs::write(root.join("lua/test.lua"), b"test").unwrap();
		std::os::unix::fs::symlink(root.join("lua/test.lua"), root.join("lua/link.lua")).unwrap();
		std::os::unix::fs::symlink(root.join("missing"), root.join("broken")).unwrap();
		std::os::unix::fs::symlink(root, root.join("loop")).unwrap();
		assert_eq!(ContentManifest::build(root, &[], || false).unwrap().entries.len(), 1);
		fs::write(root.join("lua/Test.lua"), b"duplicate").unwrap();
		assert!(matches!(ContentManifest::build(root, &[], || false), Err(GMAError::DuplicateEntry(path)) if path == "lua/test.lua"));
	}

	#[test]
	fn rebuilding_revalidates_changed_files_and_exclusions() {
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path();
		fs::create_dir(root.join("lua")).unwrap();
		fs::write(root.join("lua/test.lua"), b"test").unwrap();
		assert!(ContentManifest::build(root, &[], || false).is_ok());
		fs::write(root.join("new.exe"), b"invalid").unwrap();
		assert!(matches!(ContentManifest::build(root, &[], || false), Err(GMAError::NotWhitelisted(paths)) if paths == ["new.exe"]));
		assert!(ContentManifest::build(root, &["*.exe".to_owned()], || false).is_ok());
		assert!(matches!(
			ContentManifest::build(root, &["*".to_owned()], || false),
			Err(GMAError::NoEntries)
		));
		fs::remove_file(root.join("new.exe")).unwrap();
		fs::remove_file(root.join("lua/test.lua")).unwrap();
		assert!(matches!(ContentManifest::build(root, &[], || false), Err(GMAError::NoEntries)));
	}

	#[test]
	fn rejects_invalid_roots_and_propagates_traversal_errors() {
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().join("source");
		assert!(matches!(
			ContentManifest::build(Path::new("relative"), &[], || false),
			Err(GMAError::InvalidContentPath)
		));
		assert!(matches!(ContentManifest::build(&root, &[], || false), Err(GMAError::IOError(_))));
		fs::write(&root, b"file").unwrap();
		assert!(matches!(ContentManifest::build(&root, &[], || false), Err(GMAError::InvalidContentPath)));
		fs::remove_file(&root).unwrap();
		fs::create_dir(&root).unwrap();
		for i in 0..12 {
			fs::write(root.join(format!("{i}.exe")), b"invalid").unwrap();
		}
		let missing = root.join("missing");
		let entries = WalkDir::new(&root).into_iter().chain(WalkDir::new(&missing));
		let error = ContentManifest::collect(&root, &[], || false, entries).unwrap_err();
		assert!(matches!(error, GMAError::IOError(ref details) if details.operation == "read content directory"));
		assert!(error.to_string().contains(missing.to_str().unwrap()));
		let error = ContentManifest::build(&root, &[], || false).unwrap_err();
		assert!(matches!(error, GMAError::NotWhitelisted(paths) if paths.len() == 10 && paths[9] == "..."));
	}

	#[test]
	fn propagates_metadata_errors_for_enumerated_files() {
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path();
		fs::create_dir(root.join("lua")).unwrap();
		let source = root.join("lua/test.lua");
		fs::write(&source, b"test").unwrap();
		let entries: Vec<_> = WalkDir::new(root).into_iter().collect();
		fs::remove_file(&source).unwrap();
		let error = ContentManifest::collect(root, &[], || false, entries).unwrap_err();
		assert!(matches!(error, GMAError::IOError(ref details) if details.operation == "read source metadata" && details.path == source));
	}

	#[test]
	fn manifest_scan_can_be_cancelled() {
		let directory = tempfile::tempdir().unwrap();
		assert!(matches!(ContentManifest::build(directory.path(), &[], || true), Err(GMAError::Cancelled)));
		let mut checks = 0;
		assert!(matches!(
			ContentManifest::build(directory.path(), &[], || {
				checks += 1;
				checks > 1
			}),
			Err(GMAError::Cancelled)
		));
	}
}
