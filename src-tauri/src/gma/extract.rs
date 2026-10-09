use std::{
	fs::{self, File},
	io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
	path::{Path, PathBuf},
};

use crate::transactions::Transaction;

use super::{whitelist, workshop::WorkshopInfo, GMAEntry, GMAError, GMAFile, GMAMetadata, GMAReader};

use lazy_static::lazy_static;
use rayon::ThreadPool;
use super::{output::Directory, staging::{ExtractionStage, StagedMetadata, METADATA_LEAF}};
use serde::{Deserialize, Serialize};

lazy_static! {
	pub static ref THREAD_POOL: ThreadPool = thread_pool!(2);
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ExtractionOverwriteMode {
	Overwrite,
	#[default]
	Recycle,
	Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ExtractDestination {
	#[default]
	Temp,
	Downloads,
	Addons,
	/// path/to/addon/*
	Directory(PathBuf),
	/// path/to/addon/addon_name_123456790/*
	NamedDirectory(PathBuf),
}

#[derive(Debug, Clone, Default)]
pub struct ExtractOptions {
	pub workshop_title: bool,
	/// The metadata filename, when metadata export is enabled.
	pub metadata_name: Option<String>,
	pub workshop: Option<WorkshopInfo>,
}
impl ExtractOptions {
	pub fn from_settings() -> Self {
		let settings = app_data!().settings.read();
		Self {
			workshop_title: settings.extract_workshop_title,
			metadata_name: settings.extract_workshop_metadata.then(|| settings.extract_metadata_filename.clone()),
			workshop: None,
		}
	}

	pub fn enabled(&self) -> bool {
		self.workshop_title || self.metadata_name.is_some()
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Extracted {
	pub path: PathBuf,
	pub metadata: Option<PathBuf>,
}

fn extraction_temp_dir() -> Result<PathBuf, GMAError> {
	if let Some(path) = app_data!().settings.read().temp.clone() { return Ok(path); }
	// Resolve the OS-provided base (notably /var on macOS), then enforce no-follow
	// traversal for the application directory and every extraction beneath it.
	let base = std::env::temp_dir();
	base.canonicalize().map(|base| base.join("nwmpublisher"))
		.map_err(|error| GMAError::io("resolve system temporary directory", &base, error))
}

impl ExtractDestination {
	fn creates_folder(&self) -> bool {
		!matches!(self, Self::Directory(_))
	}

	fn prepare(self, extracted_name: &str) -> Result<(Directory, PathBuf, ExtractionOverwriteMode), GMAError> {
		let mode = if matches!(self, Self::Directory(_)) { ExtractionOverwriteMode::Overwrite } else { app_data!().settings.read().extract_overwrite_mode.clone() };
		let path = match self {
			Self::Directory(path) => path,
			Self::NamedDirectory(path) => path.join(extracted_name),
			Self::Temp => extraction_temp_dir()?.join(extracted_name),
			Self::Downloads => app_data!().downloads_dir().as_ref().ok_or_else(|| GMAError::NoSafeDestination(PathBuf::from("Downloads")))?.join(extracted_name),
			Self::Addons => app_data!().gmod_dir().ok_or_else(|| GMAError::NoSafeDestination(PathBuf::from("Addons")))?.join("GarrysMod/addons").join(extracted_name),
		};
		let absolute = std::path::absolute(&path).map_err(|error| GMAError::io("resolve extraction destination", &path, error))?;
		let name = absolute.file_name().ok_or_else(|| GMAError::NoSafeDestination(absolute.clone()))?.into();
		let parent = Directory::open(absolute.parent().ok_or(GMAError::FormatError)?, true)
			.map_err(|error| GMAError::io("open extraction parent", &absolute, error))?;
		Ok((parent, name, mode))
	}
}

impl GMAFile {
	pub fn decompress<P: AsRef<Path>>(path: P, transaction: Transaction) -> Result<GMAFile, GMAError> {
		main_thread_forbidden!();

		let path = path.as_ref();
		let input = File::open(path).map_err(|error| GMAError::io("open compressed archive", path, error))?;
		let bytes_total = input
			.metadata()
			.map_err(|error| GMAError::io("read archive metadata", path, error))?
			.len();
		let lzma_decoder = xz2::stream::Stream::new_lzma_decoder(256 * 1024 * 1024)
			.map_err(|error| GMAError::LZMA(format!("initialize decoder \"{}\": {}", path.display(), error)))?;
		let mut xz_decoder = xz2::read::XzDecoder::new_stream(input, lzma_decoder);
		let temporary = app_data!().temp_dir().to_owned();
		fs::create_dir_all(&temporary).map_err(|error| GMAError::io("create decompression directory", &temporary, error))?;
		let mut output = tempfile::NamedTempFile::new_in(&temporary).map_err(|error| GMAError::io("create decompression spool", &temporary, error))?;
		let mut decompressed_size = 0u64;
		let mut buffer = [0; 64 * 1024];
		transaction.data((turbonone!(), bytes_total));
		loop {
			if transaction.aborted() {
				return Err(GMAError::Cancelled);
			}
			let read = match xz_decoder.read(&mut buffer) {
				Ok(0) => break,
				Ok(read) => read,
				Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
				Err(error) => return Err(GMAError::io("decompress archive", path, error)),
			};
			decompressed_size = decompressed_size.checked_add(read as u64).ok_or(GMAError::FormatError)?;
			if decompressed_size > 64 * 1024 * 1024 * 1024 { return Err(GMAError::LimitExceeded("64 GiB decompressed output".into())); }
			output.write_all(&buffer[..read]).map_err(|error| GMAError::io("write decompression spool", output.path(), error))?;
			if bytes_total > 0 {
				transaction.progress(xz_decoder.total_in() as f64 / bytes_total as f64);
			}
			if decompressed_size > bytes_total {
				transaction.data((turbonone!(), decompressed_size));
			}
		}

		output.flush().map_err(|error| GMAError::io("flush decompression spool", output.path(), error))?;
		output.seek(SeekFrom::Start(0)).map_err(|error| GMAError::io("rewind decompression spool", output.path(), error))?;
		let reader = output.reopen().map_err(|error| GMAError::io("open decompression spool", output.path(), error))?;
		let mut gma = GMAFile::read_header(GMAReader::Disk(BufReader::new(reader)), path)?;
		gma.size = decompressed_size;
		gma.spool = Some(std::sync::Arc::new(output));

		Ok(gma)
	}

	fn stream_entry_bytes(
		&self,
		handle: &mut GMAReader,
		entry_path: &Path,
		entry: &GMAEntry,
		transaction: &Transaction,
		file: File,
	) -> Result<(), GMAError> {
		let offset = self.pointers.entries.checked_add(entry.index).ok_or(GMAError::FormatError)?;
		handle
			.seek(SeekFrom::Start(offset))
			.map_err(|error| GMAError::io("seek archive entry", &self.path, error))?;
		let mut writer = BufWriter::new(file);
		let mut remaining = entry.size;
		let mut buffer = [0; 64 * 1024];
		let mut hash = crc32fast::Hasher::new();
		while remaining > 0 {
			if transaction.aborted() { return Err(GMAError::Cancelled); }
			let count = remaining.min(buffer.len() as u64) as usize;
			handle.read_exact(&mut buffer[..count]).map_err(|error| GMAError::io("read archive entry", &self.path, error))?;
			writer.write_all(&buffer[..count]).map_err(|error| GMAError::io("write extracted file", entry_path, error))?;
			hash.update(&buffer[..count]);
			remaining -= count as u64;
		}
		if entry.crc != 0 && hash.finalize() != entry.crc { return Err(GMAError::Checksum(entry.path.clone())); }
		writer.flush().map_err(|error| GMAError::io("flush extracted file", entry_path, error))?;
		writer.get_ref().sync_all().map_err(|error| GMAError::io("sync extracted file", entry_path, error))?;
		Ok(())
	}
}

impl GMAFile {
	fn extract_staged(&self, destination: ExtractDestination, transaction: &Transaction, open: bool, ignore_whitelist: bool, single: Option<String>, options: &ExtractOptions) -> Result<Extracted, GMAError> {
		let result = (|| {
			if transaction.aborted() { return Err(GMAError::Cancelled); }
			self.validate_payloads(transaction)?;
			let entries = self.entries.as_ref().ok_or(GMAError::FormatError)?;
			let mut files: Vec<_> = if let Some(path) = &single {
				vec![entries.get(path).ok_or(GMAError::EntryNotFound)?]
			} else { entries.values().collect() };
			files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
			for entry in &files {
				if super::read::is_unsafe_entry_path(&entry.path) { return Err(GMAError::UnsafeEntry(entry.path.clone())); }
				if !ignore_whitelist && !whitelist::check(&entry.path) { return Err(GMAError::NotWhitelisted(vec![entry.path.clone()])); }
			}
			let metadata = if single.is_none() { self.metadata.as_ref() } else { None };
			let json = if let Some(metadata @ GMAMetadata::Standard { .. }) = metadata {
				Some(serde_json::to_vec_pretty(metadata).map_err(|error| GMAError::MetadataError(error.to_string()))?)
			} else { None };
			// Generated metadata owns addon.json, just as in the original extraction contract.
			if json.is_some() && files.iter().any(|entry| entry.path.to_lowercase().starts_with("addon.json/")) { return Err(GMAError::DuplicateEntry("addon.json".into())); }
			if json.is_some() { files.retain(|entry| !entry.path.eq_ignore_ascii_case("addon.json")); }
			let mut paths: Vec<String> = files.iter().map(|entry| entry.path.clone()).collect();
			if json.is_some() { paths.push("addon.json".into()); }
			let workshop = options.workshop.clone().unwrap_or_default();
			let name = if single.is_none() && options.workshop_title && destination.creates_folder() {
				workshop.folder_name().unwrap_or_else(|| {
					transaction.warning("ERR_WORKSHOP_FOLDER_NAME".into());
					self.extracted_name.clone()
				})
			} else { self.extracted_name.clone() };
			let workshop_file = options.metadata_name.as_ref().filter(|_| single.is_none()).map(|name| {
				let reserved = paths.iter().map(|path| path.split('/').next().unwrap().to_lowercase()).collect();
				(StagedMetadata { name: name.clone(), reserved }, workshop.render(self.metadata.as_ref().map(GMAMetadata::title)))
			});
			let (parent, name, mode) = destination.prepare(&name)?;
			let mut staging = ExtractionStage::new(parent)?;
			let extraction = (|| {
				let mut reader = self.read()?;
				for (index, entry) in files.iter().enumerate() {
					let leaf = PathBuf::from(format!("new-{index}"));
					let file = staging.directory.create_file(&leaf).map_err(|error| GMAError::io("create extracted file", &staging.directory.path.join(&leaf), error))?;
					self.stream_entry_bytes(&mut reader, &staging.directory.path.join(&leaf), entry, transaction, file)?;
					transaction.progress((index + 1) as f64 / paths.len().max(1) as f64);
				}
				if let Some(json) = json {
					let leaf = PathBuf::from(format!("new-{}", files.len()));
					let mut file = staging.directory.create_file(&leaf).map_err(|error| GMAError::io("create metadata", &staging.directory.path.join(&leaf), error))?;
					file.write_all(&json).and_then(|_| file.sync_all()).map_err(|error| GMAError::io("write metadata", &staging.directory.path.join(&leaf), error))?;
				}
				if let Some((_, text)) = &workshop_file {
					let leaf = staging.directory.path.join(METADATA_LEAF);
					let mut file = staging.directory.create_file(Path::new(METADATA_LEAF)).map_err(|error| GMAError::io("create Workshop metadata", &leaf, error))?;
					file.write_all(text.as_bytes()).and_then(|_| file.sync_all()).map_err(|error| GMAError::io("write Workshop metadata", &leaf, error))?;
				}
				if !transaction.begin_commit() { return Err(GMAError::Cancelled); }
				staging.commit(&name, mode, &paths, workshop_file.as_ref().map(|(staged, _)| staged))
			})();
			for warning in &staging.warnings { transaction.warning(warning.clone()); }
			let cleanup = staging.cleanup();
			let (path, metadata_path) = match (extraction, cleanup) {
				(Ok(extracted), Ok(())) => extracted,
				(Ok(extracted), Err(error)) => { transaction.warning(error.to_string()); extracted },
				(Err(error), Ok(())) => return Err(error),
				(Err(error), Err(cleanup)) => return Err(GMAError::MetadataError(format!("{}; {}", error, cleanup))),
			};
			if let (Some(file), Some((staged, _))) = (metadata_path.as_ref().and_then(|path| path.file_name()), &workshop_file) {
				if file != staged.name.as_str() { transaction.warning(format!("ERR_WORKSHOP_METADATA_RENAMED:{}", file.to_string_lossy())); }
			}
			Ok(Extracted { path: single.as_ref().map_or(path.clone(), |entry| path.join(entry)), metadata: metadata_path })
		})();
		match &result {
			Ok(extracted) => {
				transaction.finished(extracted.path.clone());
				if open && (single.is_some() || app_data!().settings.read().open_folder_after_extract) { crate::path::open(&extracted.path); }
			}
			Err(GMAError::Cancelled) => transaction.cancelled(),
			Err(error) => transaction.error(error.to_string(), turbonone!()),
		}
		result
	}
}

pub trait ExtractGMAImmut {
	fn extract(
		&self,
		dest: ExtractDestination,
		transaction: &Transaction,
		open_after_extract: bool,
		ignore_whitelist: bool,
		options: &ExtractOptions,
	) -> Result<Extracted, GMAError>;
	fn extract_entry(&self, entry_path: String, transaction: &Transaction, open_after_extract: bool) -> Result<PathBuf, GMAError>;
	fn extract_entry_with_handle(
		&self,
		entry_path: String,
		transaction: &Transaction,
		open_after_extract: bool,
		handle: Option<GMAReader>,
	) -> Result<PathBuf, GMAError>;
}
pub trait ExtractGMAMut {
	fn extract(
		&mut self,
		dest: ExtractDestination,
		transaction: &Transaction,
		open_after_extract: bool,
		ignore_whitelist: bool,
		options: &ExtractOptions,
	) -> Result<Extracted, GMAError>;
	fn extract_entry(&mut self, entry_path: String, transaction: &Transaction, open_after_extract: bool) -> Result<PathBuf, GMAError>;
}
impl ExtractGMAImmut for GMAFile {
	fn extract(
		&self,
		dest: ExtractDestination,
		transaction: &Transaction,
		open_after_extract: bool,
		ignore_whitelist: bool,
		options: &ExtractOptions,
	) -> Result<Extracted, GMAError> {
		THREAD_POOL.install(|| self.extract_staged(dest, transaction, open_after_extract, ignore_whitelist, None, options))
	}

	fn extract_entry_with_handle(
		&self,
		entry_path: String,
		transaction: &Transaction,
		open_after_extract: bool,
		_handle: Option<GMAReader>,
	) -> Result<PathBuf, GMAError> {
		let parent = extraction_temp_dir()?.join("nwmpublisher").join(&self.extracted_name);
		self.extract_staged(ExtractDestination::Directory(parent), transaction, open_after_extract, true, Some(entry_path), &ExtractOptions::default())
			.map(|extracted| extracted.path)
	}

	fn extract_entry(&self, entry_path: String, transaction: &Transaction, open_after_extract: bool) -> Result<PathBuf, GMAError> {
		ExtractGMAImmut::extract_entry_with_handle(self, entry_path, transaction, open_after_extract, None)
	}
}
impl ExtractGMAMut for GMAFile {
	fn extract(
		&mut self,
		dest: ExtractDestination,
		transaction: &Transaction,
		open_after_extract: bool,
		ignore_whitelist: bool,
		options: &ExtractOptions,
	) -> Result<Extracted, GMAError> {
		THREAD_POOL.install(move || {
			self.entries().inspect_err(|error| {
				if !transaction.aborted() {
					transaction.error(error.to_string(), turbonone!());
				}
			})?;
			(*self).extract(dest, transaction, open_after_extract, ignore_whitelist, options)
		})
	}
	fn extract_entry(&mut self, entry_path: String, transaction: &Transaction, open_after_extract: bool) -> Result<PathBuf, GMAError> {
		THREAD_POOL.install(move || {
			let handle = self.entries().inspect_err(|error| {
				if !transaction.aborted() {
					transaction.error(error.to_string(), turbonone!());
				}
			})?;
			(*self).extract_entry_with_handle(entry_path, transaction, open_after_extract, handle)
		})
	}
}

impl GMAFile {
	/// A Workshop ID inferred from the archive's file name or Workshop content folder.
	pub fn inferred_ws_id(&self) -> Option<steamworks::PublishedFileId> {
		self.id.or_else(|| crate::GameAddons::workshop_content_id(&self.path))
	}
}

#[tauri::command]
pub fn extract_gma(gma_path: PathBuf, dest: ExtractDestination) -> Option<u32> {
	let transaction = crate::transactions::new_extraction();
	let id = transaction.id;
	let mut options = ExtractOptions::from_settings();
	rayon::spawn(move || match GMAFile::open(gma_path) {
		Ok(mut gma) => {
			if !crate::steam::item_info::attach(&mut options, gma.inferred_ws_id(), true, None, &transaction) { return transaction.cancelled(); }
			let _ = ExtractGMAMut::extract(&mut gma, dest, &transaction, true, true, &options);
		}
		Err(error) => transaction.error(error.to_string(), turbonone!()),
	});
	Some(id)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::HashMap;

	fn fixture(name: &str, bytes: Vec<u8>) -> (GMAFile, PathBuf) {
		let root = std::env::temp_dir().canonicalize().unwrap().join(format!("nwmpublisher-{name}-{}", std::process::id()));
		fs::create_dir(&root).unwrap();
		let entry = GMAEntry {
			path: "lua/test.lua".into(),
			size: 4,
			crc: 0,
			index: 0,
		};
		let gma = GMAFile {
			path: root.join("source.gma"),
			size: bytes.len() as u64,
			id: None,
			metadata: Some(GMAMetadata::Standard {
				title: "Test".into(),
				addon_type: "tool".into(),
				tags: vec![],
				ignore: vec![],
			}),
			entries: Some(HashMap::from([(entry.path.clone(), entry)])),
			pointers: Default::default(),
			version: 3,
			extracted_name: "test".into(),
			modified: None,
			membuffer: Some(bytes.into()),
			spool: None,
		};
		(gma, root)
	}

	#[test]
	fn extraction_requires_entry_and_metadata_writes() {
		let (gma, root) = fixture("extraction-failures", b"test".to_vec());
		let destination = root.join("output");
		fs::create_dir(&destination).unwrap();
		fs::write(destination.join("lua"), b"blocks directory creation").unwrap();
		let transaction = transaction!();
		let error = gma
			.extract(ExtractDestination::Directory(destination.clone()), &transaction, false, true, &ExtractOptions::default())
			.unwrap_err();
		assert!(error.to_string().contains("create directory"));
		assert!(error.to_string().contains("lua"));
		assert!(!destination.join("addon.json").exists());
		assert!(transaction.aborted());

		fs::remove_file(destination.join("lua")).unwrap();
		fs::create_dir(destination.join("addon.json")).unwrap();
		let transaction = transaction!();
		let error = gma
			.extract(ExtractDestination::Directory(destination.clone()), &transaction, false, true, &ExtractOptions::default())
			.unwrap_err();
		assert!(matches!(error, GMAError::UnsafeEntry(_)));
		assert!(error.to_string().contains("addon.json"));
		assert!(transaction.aborted());

		fs::remove_dir(destination.join("addon.json")).unwrap();
		let transaction = transaction!();
		assert_eq!(
			gma.extract(ExtractDestination::Directory(destination.clone()), &transaction, false, true, &ExtractOptions::default())
				.unwrap(),
			Extracted { path: destination.clone(), metadata: None }
		);
		assert_eq!(fs::read(destination.join("lua/test.lua")).unwrap(), b"test");
		let metadata: serde_json::Value = serde_json::from_slice(&fs::read(destination.join("addon.json")).unwrap()).unwrap();
		assert_eq!(metadata["title"], "Test");
		fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn truncated_entry_fails_instead_of_completing() {
		let (gma, root) = fixture("truncated-extraction", b"x".to_vec());
		let transaction = transaction!();
		let error = gma
			.extract(ExtractDestination::Directory(root.join("output")), &transaction, false, true, &ExtractOptions::default())
			.unwrap_err();
		assert!(
			matches!(error, GMAError::IOError(ref details) if details.operation == "read archive entry" && details.source.kind() == std::io::ErrorKind::UnexpectedEof)
		);
		assert!(error.to_string().contains("source.gma"));
		fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn decompression_rejects_truncated_streams_even_with_a_valid_gma_header() {
		let (_, root) = fixture("compressed-extraction", Vec::new());
		let path = root.join("archive.bin");
		let options = xz2::stream::LzmaOptions::new_preset(1).unwrap();
		let stream = xz2::stream::Stream::new_lzma_encoder(&options).unwrap();
		let mut encoder = xz2::write::XzEncoder::new_stream(Vec::new(), stream);
		let mut bytes = b"GMAD\x03".to_vec();
		bytes.resize(1024, 0);
		encoder.write_all(&bytes).unwrap();
		let compressed = encoder.finish().unwrap();
		fs::write(&path, &compressed).unwrap();
		let transaction = transaction!();
		assert_eq!(GMAFile::decompress(&path, transaction.clone()).unwrap().size, bytes.len() as u64);
		transaction.cancel();

		fs::write(&path, &compressed[..compressed.len() - 6]).unwrap();
		let transaction = transaction!();
		let error = GMAFile::decompress(&path, transaction.clone()).unwrap_err();
		assert!(error.to_string().contains("decompress archive"));
		assert!(error.to_string().contains("archive.bin"));
		transaction.cancel();
		fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn empty_archive_writes_metadata_and_cancelled_job_does_not_write() {
		let (mut gma, root) = fixture("empty-extraction", Vec::new());
		gma.entries.as_mut().unwrap().clear();
		let transaction = transaction!();
		gma.extract(ExtractDestination::Directory(root.join("output")), &transaction, false, true, &ExtractOptions::default())
			.unwrap();
		assert!(root.join("output/addon.json").is_file());
		let transaction = transaction!();
		transaction.cancel();
		assert!(matches!(
			gma.extract(ExtractDestination::Directory(root.join("cancelled")), &transaction, false, true, &ExtractOptions::default()),
			Err(GMAError::Cancelled)
		));
		assert!(!root.join("cancelled").exists());
		fs::remove_dir_all(root).unwrap();
	}

	fn workshop(id: u64, title: Option<&str>) -> WorkshopInfo {
		WorkshopInfo {
			id: Some(steamworks::PublishedFileId(id)),
			id_inferred: false,
			item: title.map(|title| super::super::workshop::WorkshopItemDetails {
				title: title.into(),
				description: "[b]Description[/b]".into(),
				owner: steamworks::SteamId::from_raw(76561197960287930),
				retrieved: chrono::Utc::now(),
			}),
			owner_name: Some("Owner".into()),
		}
	}

	fn names(path: &Path) -> Vec<String> {
		let mut names: Vec<_> = fs::read_dir(path).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
		names.sort();
		names
	}

	#[test]
	fn workshop_folder_names_apply_only_to_generated_folders() {
		let (gma, root) = fixture("workshop-folder-names", b"test".to_vec());
		let parent = root.join("parent");
		let extract = |destination, options: &ExtractOptions| gma.extract(destination, &transaction!(), false, true, options).unwrap();
		assert_eq!(extract(ExtractDestination::NamedDirectory(parent.clone()), &ExtractOptions::default()), Extracted { path: parent.join("test"), metadata: None });
		assert_eq!(names(&parent.join("test")), ["addon.json", "lua"]);

		let named = |id, title| ExtractOptions { workshop_title: true, workshop: Some(workshop(id, title)), ..Default::default() };
		assert_eq!(extract(ExtractDestination::NamedDirectory(parent.clone()), &named(1, Some("Café: Addon"))).path, parent.join("Café Addon [1]"));
		assert_eq!(extract(ExtractDestination::NamedDirectory(parent.clone()), &named(2, Some("Café: Addon"))).path, parent.join("Café Addon [2]"));
		assert_eq!(extract(ExtractDestination::NamedDirectory(parent.clone()), &named(3, None)).path, parent.join("test"));
		let exact = root.join("exact");
		assert_eq!(extract(ExtractDestination::Directory(exact.clone()), &named(1, Some("Café: Addon"))).path, exact);
		let (retained, entries): (Vec<_>, Vec<_>) = names(&parent).into_iter().partition(|name| name.starts_with(".nwmpublisher-extract-"));
		// Unix keeps the replaced "test" folder for recovery instead of recycling it.
		assert_eq!(retained.len(), usize::from(cfg!(unix)));
		assert!(retained.iter().all(|name| parent.join(name).join("previous/lua/test.lua").is_file()));
		assert_eq!(entries, ["Café Addon [1]", "Café Addon [2]", "test"]);
		assert_eq!(fs::read(parent.join("Café Addon [2]/lua/test.lua")).unwrap(), b"test");
		fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn metadata_is_numbered_around_archive_content_and_existing_entries() {
		let (mut gma, root) = fixture("workshop-metadata", b"testarchive".to_vec());
		gma.entries.as_mut().unwrap().insert("workshop.txt".into(), GMAEntry { path: "workshop.txt".into(), size: 7, crc: 0, index: 4 });
		let destination = root.join("output");
		let options = ExtractOptions { metadata_name: Some("workshop.txt".into()), workshop: Some(workshop(42, Some("Addon"))), ..Default::default() };
		let extracted = gma.extract(ExtractDestination::Directory(destination.clone()), &transaction!(), false, true, &options).unwrap();
		assert_eq!(extracted.metadata, Some(destination.join("workshop (2).txt")));
		assert_eq!(fs::read(destination.join("workshop.txt")).unwrap(), b"archive");
		let text = fs::read_to_string(destination.join("workshop (2).txt")).unwrap();
		assert!(text.starts_with("Title: Addon\nWorkshop ID: 42\n"));
		assert!(text.ends_with("Description:\n[b]Description[/b]\n"));

		fs::create_dir(destination.join("workshop (3).txt")).unwrap();
		let extracted = gma.extract(ExtractDestination::Directory(destination.clone()), &transaction!(), false, true, &options).unwrap();
		assert_eq!(extracted.metadata, Some(destination.join("workshop (4).txt")));
		assert_eq!(fs::read_to_string(destination.join("workshop (2).txt")).unwrap(), text);
		assert!(destination.join("workshop (3).txt").is_dir());

		let disabled = gma.extract(ExtractDestination::Directory(root.join("disabled")), &transaction!(), false, true, &ExtractOptions::default()).unwrap();
		assert_eq!(disabled.metadata, None);
		assert_eq!(names(&root.join("disabled")), ["addon.json", "lua", "workshop.txt"]);
		fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn metadata_failure_rolls_back_extracted_files() {
		let (gma, root) = fixture("workshop-metadata-rollback", b"test".to_vec());
		let destination = root.join("output");
		fs::create_dir_all(destination.join("lua")).unwrap();
		fs::write(destination.join("lua/test.lua"), b"original").unwrap();
		for number in 1..=255 { fs::write(destination.join(super::super::filename::numbered_metadata_file_name("info.txt", number)), b"keep").unwrap(); }
		let options = ExtractOptions { metadata_name: Some("info.txt".into()), ..Default::default() };
		let transaction = transaction!();
		let error = gma.extract(ExtractDestination::Directory(destination.clone()), &transaction, false, true, &options).unwrap_err();
		assert!(matches!(error, GMAError::NoSafeDestination(_)));
		assert!(transaction.aborted());
		assert_eq!(fs::read(destination.join("lua/test.lua")).unwrap(), b"original");
		assert!(!destination.join("addon.json").exists());
		assert_eq!(fs::read(destination.join("info.txt")).unwrap(), b"keep");
		assert_eq!(names(&destination).len(), 256);

		let fresh = root.join("fresh");
		let options = ExtractOptions { metadata_name: Some("info.txt".into()), workshop_title: true, ..Default::default() };
		let extracted = gma.extract(ExtractDestination::NamedDirectory(fresh.clone()), &transaction!(), false, true, &options).unwrap();
		assert_eq!(extracted, Extracted { path: fresh.join("test"), metadata: Some(fresh.join("test/info.txt")) });
		assert!(fs::read_to_string(fresh.join("test/info.txt")).unwrap().starts_with("Title: Unavailable\nArchive title: Test\nWorkshop ID: Unavailable\n"));
		fs::remove_dir_all(root).unwrap();
	}
}
