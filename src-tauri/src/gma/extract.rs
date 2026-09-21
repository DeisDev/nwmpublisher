use std::{
	fs::{self, File},
	io::{BufWriter, Cursor, Read, SeekFrom, Write},
	path::{Path, PathBuf},
	sync::atomic::{AtomicUsize, Ordering},
};

use crate::transactions::Transaction;

use super::{whitelist, GMAEntry, GMAError, GMAFile, GMAMetadata, GMAReader};

use lazy_static::lazy_static;
use rayon::{
	iter::{IntoParallelRefIterator, ParallelIterator},
	ThreadPool,
};
use serde::{Deserialize, Serialize};

lazy_static! {
	pub static ref THREAD_POOL: ThreadPool = thread_pool!();
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
impl ExtractDestination {
	fn prepare<S: AsRef<str>>(self, extracted_name: S) -> PathBuf {
		use ExtractDestination::*;

		let push_extracted_name = |mut path: PathBuf| {
			path.push(extracted_name.as_ref());
			Some(path)
		};

		let recycle_existing = !matches!(self, Directory(_));

		let mut path = match self {
			Temp => None,

			Directory(path) => Some(path),

			Addons => app_data!().gmod_dir().map(|mut path| {
				path.push("GarrysMod");
				path.push("addons");
				path.push(extracted_name.as_ref());
				path
			}),

			Downloads => app_data!().downloads_dir().to_owned().and_then(push_extracted_name),

			NamedDirectory(path) => push_extracted_name(path),
		}
		.unwrap_or_else(|| push_extracted_name(app_data!().temp_dir().to_owned()).unwrap());

		if recycle_existing && path.exists() {
			let success = match &app_data!().settings.read().extract_overwrite_mode {
				ExtractionOverwriteMode::Overwrite => true,
				ExtractionOverwriteMode::Recycle => trash::delete(&path).is_ok(),
				ExtractionOverwriteMode::Delete => fs::remove_dir_all(&path).is_ok(),
			};
			if !success {
				let dir_name = path.file_name().unwrap().to_string_lossy().to_string();
				path.pop();

				let mut i: u8 = 0;
				while i < 255 {
					i += 1;

					path.push(format!("{} ({})", dir_name, i));

					if !path.exists() {
						break;
					} else {
						path.pop();
					}
				}
			}
		}

		path
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
		let lzma_decoder = xz2::stream::Stream::new_lzma_decoder(u64::MAX)
			.map_err(|error| GMAError::LZMA(format!("initialize decoder \"{}\": {}", path.display(), error)))?;
		let mut xz_decoder = xz2::read::XzDecoder::new_stream(input, lzma_decoder);
		let mut output = Vec::new();
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
			output.extend_from_slice(&buffer[..read]);
			if bytes_total > 0 {
				transaction.progress(xz_decoder.total_in() as f64 / bytes_total as f64);
			}
			if output.len() as u64 > bytes_total {
				transaction.data((turbonone!(), output.len() as u64));
			}
		}

		output.shrink_to_fit();

		let decompressed_size = output.len() as u64;

		let mut gma = GMAFile::read_header(GMAReader::MemBuffer(Cursor::new(output.into())), path)?;
		gma.size = decompressed_size;

		Ok(gma)
	}

	fn stream_entry_bytes(
		&self,
		handle: &mut GMAReader,
		entry_path: &Path,
		entry: &GMAEntry,
		transaction: Option<&Transaction>,
	) -> Result<(), GMAError> {
		let parent = entry_path.parent().ok_or(GMAError::FormatError)?;
		fs::create_dir_all(parent).map_err(|error| GMAError::io("create directory", parent, error))?;
		let file = File::create(entry_path).map_err(|error| GMAError::io("create extracted file", entry_path, error))?;
		let offset = self.pointers.entries.checked_add(entry.index).ok_or(GMAError::FormatError)?;
		handle
			.seek(SeekFrom::Start(offset))
			.map_err(|error| GMAError::io("seek archive entry", &self.path, error))?;
		let mut writer = BufWriter::new(file);
		crate::stream_bytes(&mut **handle, &mut writer, entry.size, |written| {
			if let Some(transaction) = transaction {
				transaction.progress(written as f64 / entry.size as f64);
			}
		})
		.map_err(|error| match error {
			crate::StreamError::Read(error) => GMAError::io("read archive entry", &self.path, error),
			crate::StreamError::Write(error) => GMAError::io("write extracted file", entry_path, error),
		})?;
		writer.flush().map_err(|error| GMAError::io("flush extracted file", entry_path, error))?;
		Ok(())
	}
}

pub trait ExtractGMAImmut {
	fn extract(
		&self,
		dest: ExtractDestination,
		transaction: &Transaction,
		open_after_extract: bool,
		ignore_whitelist: bool,
	) -> Result<PathBuf, GMAError>;
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
	) -> Result<PathBuf, GMAError>;
	fn extract_entry(&mut self, entry_path: String, transaction: &Transaction, open_after_extract: bool) -> Result<PathBuf, GMAError>;
}
impl ExtractGMAImmut for GMAFile {
	fn extract(
		&self,
		dest: ExtractDestination,
		transaction: &Transaction,
		open_after_extract: bool,
		ignore_whitelist: bool,
	) -> Result<PathBuf, GMAError> {
		let result = THREAD_POOL.install(move || {
			if transaction.aborted() {
				return Err(GMAError::Cancelled);
			}
			let dest_path = dest.prepare(&self.extracted_name);
			let entries = self.entries.as_ref().ok_or(GMAError::FormatError)?;
			let metadata = self.metadata.as_ref().ok_or(GMAError::FormatError)?;
			self.read()?;
			fs::create_dir_all(&dest_path).map_err(|error| GMAError::io("create extraction directory", &dest_path, error))?;
			let completed = AtomicUsize::new(0);
			entries.par_iter().try_for_each(|(entry_path, entry)| -> Result<(), GMAError> {
				if transaction.aborted() {
					return Err(GMAError::Cancelled);
				}
				if !ignore_whitelist && !whitelist::check(entry_path) {
					transaction.error("ERR_WHITELIST", entry_path.clone());
					return Err(GMAError::Cancelled);
				}
				let final_path = dest_path.join(entry_path);
				if !final_path.starts_with(&dest_path) {
					return Err(GMAError::FormatError);
				}
				let mut handle = self.read()?;
				self.stream_entry_bytes(&mut handle, &final_path, entry, None)?;
				let completed = completed.fetch_add(1, Ordering::AcqRel) + 1;
				transaction.progress(completed as f64 / (entries.len() + 1) as f64);
				Ok(())
			})?;
			if transaction.aborted() {
				return Err(GMAError::Cancelled);
			}
			if let GMAMetadata::Standard { .. } = metadata {
				let path = dest_path.join("addon.json");
				let json = serde_json::to_vec_pretty(metadata)
					.map_err(|error| GMAError::MetadataError(format!("serialize metadata \"{}\": {}", path.display(), error)))?;
				let file = File::create(&path).map_err(|error| GMAError::io("create metadata", &path, error))?;
				let mut writer = BufWriter::new(file);
				writer.write_all(&json).map_err(|error| GMAError::io("write metadata", &path, error))?;
				writer.flush().map_err(|error| GMAError::io("flush metadata", &path, error))?;
			}
			if transaction.aborted() {
				return Err(GMAError::Cancelled);
			}
			transaction.finished(dest_path.clone());
			if open_after_extract {
				crate::path::open(&dest_path);
			}
			Ok(dest_path)
		});

		if !transaction.aborted() {
			if let Err(ref error) = result {
				transaction.error(error.to_string(), turbonone!());
			}
		}

		result
	}

	fn extract_entry_with_handle(
		&self,
		entry_path: String,
		transaction: &Transaction,
		open_after_extract: bool,
		handle: Option<GMAReader>,
	) -> Result<PathBuf, GMAError> {
		let result = (|| -> Result<PathBuf, GMAError> {
			if transaction.aborted() {
				return Err(GMAError::Cancelled);
			}
			let mut base = app_data!().temp_dir().to_owned();
			base.push("nwmpublisher");
			base.push(&self.extracted_name);

			let mut path = base.clone();
			path.push(&entry_path);

			if !path.starts_with(&base) {
				return Err(GMAError::FormatError);
			}

			let mut handle = match handle {
				Some(handle) => handle,
				None => self.read()?,
			};

			let entry = self
				.entries
				.as_ref()
				.expect("Expected entries to be read by this point")
				.get(&entry_path)
				.ok_or(GMAError::EntryNotFound)?;

			self.stream_entry_bytes(&mut handle, &path, entry, Some(transaction))?;
			if transaction.aborted() {
				return Err(GMAError::Cancelled);
			}
			Ok(path)
		})();

		if let Err(ref error) = result {
			if !transaction.aborted() {
				transaction.error(error.to_string(), turbonone!());
			}
		} else if let Ok(ref path) = result {
			if !transaction.aborted() {
				transaction.finished(path.to_owned());
				if open_after_extract {
					crate::path::open(path);
				}
			}
		}

		result
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
	) -> Result<PathBuf, GMAError> {
		THREAD_POOL.install(move || {
			self.entries().inspect_err(|error| {
				if !transaction.aborted() {
					transaction.error(error.to_string(), turbonone!());
				}
			})?;
			(*self).extract(dest, transaction, open_after_extract, ignore_whitelist)
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

#[tauri::command]
pub fn extract_gma(gma_path: PathBuf, dest: ExtractDestination) -> Option<u32> {
	let transaction = transaction!();
	let id = transaction.id;
	rayon::spawn(move || match GMAFile::open(gma_path) {
		Ok(mut gma) => {
			let _ = ExtractGMAMut::extract(&mut gma, dest, &transaction, true, true);
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
		let root = std::env::temp_dir().join(format!("nwmpublisher-{name}-{}", std::process::id()));
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
			.extract(ExtractDestination::Directory(destination.clone()), &transaction, false, true)
			.unwrap_err();
		assert!(error.to_string().contains("create directory"));
		assert!(error.to_string().contains("lua"));
		assert!(!destination.join("addon.json").exists());
		assert!(transaction.aborted());

		fs::remove_file(destination.join("lua")).unwrap();
		fs::create_dir(destination.join("addon.json")).unwrap();
		let transaction = transaction!();
		let error = gma
			.extract(ExtractDestination::Directory(destination.clone()), &transaction, false, true)
			.unwrap_err();
		assert!(error.to_string().contains("create metadata"));
		assert!(error.to_string().contains("addon.json"));
		assert!(transaction.aborted());

		fs::remove_dir(destination.join("addon.json")).unwrap();
		let transaction = transaction!();
		assert_eq!(
			gma.extract(ExtractDestination::Directory(destination.clone()), &transaction, false, true)
				.unwrap(),
			destination
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
			.extract(ExtractDestination::Directory(root.join("output")), &transaction, false, true)
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
		gma.extract(ExtractDestination::Directory(root.join("output")), &transaction, false, true)
			.unwrap();
		assert!(root.join("output/addon.json").is_file());
		let transaction = transaction!();
		transaction.cancel();
		assert!(matches!(
			gma.extract(ExtractDestination::Directory(root.join("cancelled")), &transaction, false, true),
			Err(GMAError::Cancelled)
		));
		assert!(!root.join("cancelled").exists());
		fs::remove_dir_all(root).unwrap();
	}
}
