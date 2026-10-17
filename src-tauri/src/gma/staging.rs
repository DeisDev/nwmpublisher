use std::{collections::HashSet, io, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};
use super::{filename::numbered_file_name, output::{Directory, Kind}, ExtractionOverwriteMode, GMAError};

static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);
pub(super) const METADATA_LEAF: &str = "new-metadata";
pub(super) const ICON_LEAF: &str = "new-icon";

/// A generated file, such as Workshop metadata, added beside the archive's files.
pub(super) struct StagedFile {
	pub leaf: &'static str,
	pub name: String,
	/// Lowercase top-level archive names that the file must not take.
	pub reserved: HashSet<String>,
}

pub(super) struct ExtractionStage {
	pub directory: Directory,
	parent: Directory,
	name: PathBuf,
	preserve: bool,
	pub warnings: Vec<String>,
}

impl ExtractionStage {
	pub fn new(parent: Directory) -> Result<Self, GMAError> {
		let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
		for _ in 0..255 {
			let name = PathBuf::from(format!(".nwmpublisher-extract-{}-{}-{}", std::process::id(), nonce, NEXT_STAGE.fetch_add(1, Ordering::Relaxed)));
			match parent.mkdir(&name) {
				Ok(()) => {
					let directory = parent.child(&name, false).map_err(|error| GMAError::io("open extraction staging", &parent.path.join(&name), error))?;
					return Ok(Self { directory, parent, name, preserve: false, warnings: Vec::new() });
				}
				Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {},
				Err(error) => return Err(GMAError::io("create extraction staging", &parent.path.join(&name), error)),
			}
		}
		Err(GMAError::NoSafeDestination(parent.path))
	}

	pub fn cleanup(self) -> Result<(), GMAError> {
		let Self { directory, parent, name, preserve, .. } = self;
		let path = directory.path.clone();
		drop(directory);
		if preserve { return Ok(()); }
		parent.remove_tree(&name).map_err(|error| GMAError::io("clean extraction staging (retained for recovery)", &path, error))
	}

	pub fn commit(&mut self, name: &Path, mode: ExtractionOverwriteMode, files: &[String], generated: &[&StagedFile]) -> Result<(PathBuf, Vec<PathBuf>), GMAError> {
		let mut name = name.to_owned();
		let original = self.parent.path.join(&name);
		let kind = self.parent.kind(&name).map_err(|error| GMAError::io("inspect extraction destination", &original, error))?;
		if kind == Some(Kind::File) { return Err(GMAError::UnsafeEntry(original.display().to_string())); }
		let mut previous = false;
		let mut created = false;
		if kind.is_some() && !matches!(mode, ExtractionOverwriteMode::Overwrite) {
			match self.parent.rename(&name, &self.directory, Path::new("previous")) {
				Ok(()) => previous = true,
				Err(_) => {
					name = reserve_alternative(&self.parent, &name)?;
					created = true;
				}
			}
		}
		if !created && (kind.is_none() || previous) {
			if let Err(error) = self.parent.mkdir(&name) {
				if previous {
					self.directory.rename(Path::new("previous"), &self.parent, &name).map_err(|restore| {
						self.preserve = true;
						GMAError::MetadataError(format!("{}; rollback failed: {}; recovery: {}", error, restore, self.directory.path.display()))
					})?;
				}
				return Err(GMAError::io("create extraction destination", &original, error));
			}
			created = true;
		}
		let result = match self.parent.child(&name, false) {
			Ok(target) => self.merge(&target, files, generated),
			Err(error) => Err(GMAError::io("open extraction destination", &original, error)),
		};
		let generated = match result {
			Ok(generated) => generated,
			Err(error) => {
				if created && !self.preserve {
					if let Err(cleanup) = self.parent.remove_tree(&name) {
						self.preserve = true;
						return Err(GMAError::MetadataError(format!("{}; rollback failed: {}; recovery: {}", error, cleanup, self.directory.path.display())));
					}
				}
				if previous && !self.preserve {
					if let Err(restore) = self.directory.rename(Path::new("previous"), &self.parent, &name) {
						self.preserve = true;
						return Err(GMAError::MetadataError(format!("{}; rollback failed: {}; recovery: {}", error, restore, self.directory.path.display())));
					}
				}
				return Err(error);
			}
		};
		if previous && matches!(mode, ExtractionOverwriteMode::Recycle) {
			// The Windows ancestor handles pin this path throughout the recycle call.
			#[cfg(windows)]
			if let Err(error) = trash::delete(self.directory.path.join("previous")) {
				self.preserve = true;
				self.warnings.push(format!("{}; previous files: {}", error, self.directory.path.join("previous").display()));
			}
			// trash has no descriptor-relative Unix API. Retain the old tree instead of
			// passing a replaceable ancestor path to a recursive external operation.
			#[cfg(unix)] {
				self.preserve = true;
				self.warnings.push(format!("ERR_RECYCLE_RETAINED:{}", self.directory.path.join("previous").display()));
			}
		}
		let path = self.parent.path.join(name);
		Ok((path.clone(), generated.into_iter().map(|name| path.join(name)).collect()))
	}

	fn merge(&mut self, target: &Directory, files: &[String], generated: &[&StagedFile]) -> Result<Vec<String>, GMAError> {
		// Journal relative paths, not directory handles: one open handle per file exceeds
		// the open file limit on large addons.
		let mut journal: Vec<(&Path, bool, bool)> = Vec::new();
		let mut directories: Vec<PathBuf> = Vec::new();
		let mut generated_files: Vec<PathBuf> = Vec::new();
		let result = (|| -> Result<Vec<String>, GMAError> {
			for (index, path) in files.iter().enumerate() {
				let mut parent = target.clone();
				let mut relative = PathBuf::new();
				let path = Path::new(path);
				for part in path.parent().unwrap().components() {
					let part = PathBuf::from(part.as_os_str());
					relative.push(&part);
					if parent.kind(&part).map_err(|error| GMAError::io("inspect extraction directory", &parent.path.join(&part), error))?.is_none() {
						parent.mkdir(&part).map_err(|error| GMAError::io("create directory", &parent.path.join(&part), error))?;
						directories.push(relative.clone());
					}
					parent = parent.child(&part, false).map_err(|error| GMAError::io("create directory", &parent.path.join(&part), error))?;
				}
				let leaf = PathBuf::from(path.file_name().unwrap());
				let backup = PathBuf::from(format!("old-{index}"));
				let kind = parent.kind(&leaf).map_err(|error| GMAError::io("inspect extraction target", &parent.path.join(&leaf), error))?;
				if kind == Some(Kind::Directory) { return Err(GMAError::UnsafeEntry(parent.path.join(&leaf).display().to_string())); }
				if kind.is_some() {
					parent.rename(&leaf, &self.directory, &backup).map_err(|error| GMAError::io("back up extracted file", &parent.path.join(&leaf), error))?;
				}
				journal.push((path, kind.is_some(), false));
				self.directory.rename(Path::new(&format!("new-{index}")), &parent, &leaf)
					.map_err(|error| GMAError::io("commit extracted file", &parent.path.join(&leaf), error))?;
				journal.last_mut().unwrap().2 = true;
			}
			let mut names = Vec::new();
			'generated: for generated in generated {
				let staged = self.directory.path.join(generated.leaf);
				let mut source = self.directory.read_file(Path::new(generated.leaf)).map_err(|error| GMAError::io("read staged Workshop file", &staged, error))?;
				for number in 1..=255 {
					let name = numbered_file_name(&generated.name, number);
					if generated.reserved.contains(&name.to_lowercase()) { continue; }
					let leaf = PathBuf::from(&name);
					match target.kind(&leaf) {
						Ok(None) => {}
						// Links and other non-regular entries are reported as InvalidInput.
						Ok(Some(_)) => continue,
						Err(error) if error.kind() == io::ErrorKind::InvalidInput => continue,
						Err(error) => return Err(GMAError::io("inspect Workshop file target", &target.path.join(&leaf), error)),
					}
					// Exclusive creation also covers entries created after the check. A rename
					// could replace them.
					let mut file = match target.create_file(&leaf) {
						Ok(file) => file,
						Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
						Err(error) => return Err(GMAError::io("create Workshop file", &target.path.join(&leaf), error)),
					};
					generated_files.push(leaf);
					io::copy(&mut source, &mut file).and_then(|_| file.sync_all()).map_err(|error| GMAError::io("write Workshop file", &target.path.join(&name), error))?;
					names.push(name);
					continue 'generated;
				}
				return Err(GMAError::NoSafeDestination(target.path.join(&generated.name)));
			}
			Ok(names)
		})();
		if let Err(error) = result {
			let rollback = (|| -> io::Result<()> {
				for leaf in generated_files.drain(..).rev() { target.remove(&leaf, false)?; }
				for (index, (path, existed, installed)) in journal.iter().enumerate().rev() {
					let parent = target.descendant(path.parent().unwrap())?;
					let leaf = Path::new(path.file_name().unwrap());
					if *installed { parent.rename(leaf, &self.directory, Path::new(&format!("new-{index}")))?; }
					if *existed { self.directory.rename(Path::new(&format!("old-{index}")), &parent, leaf)?; }
				}
				while let Some(directory) = directories.pop() {
					target.descendant(directory.parent().unwrap())?.remove(Path::new(directory.file_name().unwrap()), true)?;
				}
				Ok(())
			})();
			if let Err(rollback) = rollback {
				self.preserve = true;
				return Err(GMAError::MetadataError(format!("{}; rollback failed: {}; recovery: {}", error, rollback, self.directory.path.display())));
			}
			return Err(error);
		}
		result
	}
}

fn reserve_alternative(parent: &Directory, name: &Path) -> Result<PathBuf, GMAError> {
	for index in 1..=255 {
		let candidate = PathBuf::from(format!("{} ({})", name.to_string_lossy(), index));
		match parent.mkdir(&candidate) {
			Ok(()) => return Ok(candidate),
			Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {},
			Err(error) => return Err(GMAError::io("reserve extraction destination", &parent.path.join(&candidate), error)),
		}
	}
	Err(GMAError::NoSafeDestination(parent.path.join(name)))
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Write;

	#[test]
	fn merge_failure_restores_every_replaced_file() {
		let root = tempfile::tempdir().unwrap();
		std::fs::create_dir(root.path().join("output")).unwrap();
		std::fs::write(root.path().join("output/a"), b"original").unwrap();
		std::fs::create_dir(root.path().join("output/blocked")).unwrap();
		let parent = Directory::open(&root.path().canonicalize().unwrap(), false).unwrap();
		let mut stage = ExtractionStage::new(parent).unwrap();
		stage.directory.create_file(Path::new("new-0")).unwrap().write_all(b"replacement").unwrap();
		stage.directory.create_file(Path::new("new-1")).unwrap().write_all(b"blocked").unwrap();
		assert!(stage.commit(Path::new("output"), ExtractionOverwriteMode::Overwrite, &["a".into(), "blocked".into()], &[]).is_err());
		assert_eq!(std::fs::read(root.path().join("output/a")).unwrap(), b"original");
		assert!(root.path().join("output/blocked").is_dir());
		stage.cleanup().unwrap();
	}
	#[test]
	fn merge_failure_restores_nested_files_and_removes_created_directories() {
		let root = tempfile::tempdir().unwrap();
		std::fs::create_dir_all(root.path().join("output/lua/autorun")).unwrap();
		std::fs::write(root.path().join("output/lua/autorun/a.lua"), b"original").unwrap();
		std::fs::create_dir(root.path().join("output/blocked")).unwrap();
		let parent = Directory::open(&root.path().canonicalize().unwrap(), false).unwrap();
		let mut stage = ExtractionStage::new(parent).unwrap();
		stage.directory.create_file(Path::new("new-0")).unwrap().write_all(b"replacement").unwrap();
		stage.directory.create_file(Path::new("new-1")).unwrap().write_all(b"added").unwrap();
		stage.directory.create_file(Path::new("new-2")).unwrap().write_all(b"blocked").unwrap();
		let files = ["lua/autorun/a.lua".into(), "materials/models/b.vmt".into(), "blocked".into()];
		assert!(stage.commit(Path::new("output"), ExtractionOverwriteMode::Overwrite, &files, &[]).is_err());
		assert_eq!(std::fs::read(root.path().join("output/lua/autorun/a.lua")).unwrap(), b"original");
		assert!(!root.path().join("output/materials").exists());
		assert!(root.path().join("output/blocked").is_dir());
		stage.cleanup().unwrap();
	}
	#[test]
	fn exhausted_alternatives_never_return_parent() {
		let root = tempfile::tempdir().unwrap();
		let directory = Directory::open(&root.path().canonicalize().unwrap(), false).unwrap();
		for index in 1..=255 { directory.mkdir(Path::new(&format!("addon ({index})"))).unwrap(); }
		assert!(matches!(reserve_alternative(&directory, Path::new("addon")), Err(GMAError::NoSafeDestination(_))));
	}

	#[cfg(unix)]
	#[test]
	fn directory_and_file_links_are_rejected() {
		let root = tempfile::tempdir().unwrap();
		let outside = tempfile::tempdir().unwrap();
		std::fs::write(outside.path().join("keep"), b"original").unwrap();
		std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
		std::os::unix::fs::symlink(outside.path().join("keep"), root.path().join("file")).unwrap();
		let directory = Directory::open(&root.path().canonicalize().unwrap(), false).unwrap();
		assert!(directory.child(Path::new("link"), false).is_err());
		assert!(directory.kind(Path::new("file")).is_err());
		assert_eq!(std::fs::read(outside.path().join("keep")).unwrap(), b"original");
	}
}
