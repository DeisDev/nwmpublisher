use byteorder::{LittleEndian, WriteBytesExt};
use std::{
	fs::File,
	io::{BufWriter, Read, Seek, Write},
	path::{Path, PathBuf},
	time::SystemTime,
};

use crate::{transactions::Transaction, GMAFile, NTStringWriter};

use super::{manifest::ContentManifest, GMAError, GMAMetadata};

use super::GMA_HEADER;

impl NTStringWriter for BufWriter<File> {}

const PACK_CHUNK_SIZE: usize = 64 * 1024;

fn check_cancelled(transaction: &Transaction) -> Result<(), GMAError> {
	if transaction.aborted() {
		Err(GMAError::Cancelled)
	} else {
		Ok(())
	}
}

fn read_contents(reader: &mut impl Read, writer: &mut impl Write, source: &Path, output: &Path, transaction: &Transaction) -> Result<(u64, u32), GMAError> {
	let mut buffer = [0; PACK_CHUNK_SIZE];
	let mut size = 0u64;
	let mut crc32 = crc32fast::Hasher::new();
	loop {
		check_cancelled(transaction)?;
		let read = match reader.read(&mut buffer) {
			Ok(0) => break,
			Ok(read) => read,
			Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
			Err(error) => return Err(GMAError::io("read source file", source, error)),
		};
		check_cancelled(transaction)?;
		writer.write_all(&buffer[..read]).map_err(|error| GMAError::io("write archive", output, error))?;
		crc32.update(&buffer[..read]);
		size = size.checked_add(read as u64).ok_or(GMAError::FormatError)?;
	}
	Ok((size, crc32.finalize()))
}

impl GMAFile {
	pub fn write(&self) -> Result<BufWriter<File>, GMAError> {
		Ok(BufWriter::new(
			File::create(&self.path).map_err(|error| GMAError::io("create archive", &self.path, error))?,
		))
	}

	pub fn create(&self, manifest: ContentManifest, transaction: Transaction) -> Result<(), GMAError> {
		check_cancelled(&transaction)?;
		let mut f = self.write()?;

		let metadata = self.metadata.as_ref().expect("Expected metadata to be set");

		let (title, addon_json) = match metadata {
			GMAMetadata::Legacy { title, .. } => (title.as_str(), None),
			GMAMetadata::Standard { title, .. } => (title.as_str(), Some(metadata)),
		};

		f.write_all(GMA_HEADER)
			.map_err(|error| GMAError::io("write archive", &self.path, error))?;

		f.write_u8(3).map_err(|error| GMAError::io("write archive", &self.path, error))?; // gma version

		// steamid [unused]
		f.write_u64::<LittleEndian>(0)
			.map_err(|error| GMAError::io("write archive", &self.path, error))?;

		// timestamp [unused]
		f.write_u64::<LittleEndian>(match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
			Ok(unix) => unix.as_secs(),
			Err(_) => 0,
		})
		.map_err(|error| GMAError::io("write archive", &self.path, error))?;

		// required content [unused]
		f.write_u8(0).map_err(|error| GMAError::io("write archive", &self.path, error))?;

		// addon name
		f.write_nt_string(title)
			.map_err(|error| GMAError::io("write archive", &self.path, error))?;

		// addon description
		match addon_json {
			Some(addon_json) => f
				.write_nt_string(
					serde_json::ser::to_string(addon_json)
						.map_err(|error| GMAError::MetadataError(format!("serialize archive metadata \"{}\": {}", self.path.display(), error)))?,
				)
				.map_err(|error| GMAError::io("write archive", &self.path, error))?,
			None => f
				.write_nt_string("Description")
				.map_err(|error| GMAError::io("write archive", &self.path, error))?,
		};

		// addon author [unused]
		f.write_nt_string("Author Name")
			.map_err(|error| GMAError::io("write archive", &self.path, error))?;

		// addon version [unused]
		f.write_i32::<LittleEndian>(1)
			.map_err(|error| GMAError::io("write archive", &self.path, error))?;

		// One payload buffer is reused for the entire job. The archive itself is the spool.
		let mut entries = manifest.into_entries();
		entries.sort_unstable_by(|a, b| a.archive_path.cmp(&b.archive_path));
		let mut positions = Vec::with_capacity(entries.len());
		for (index, entry) in entries.iter().enumerate() {
			check_cancelled(&transaction)?;
			f.write_u32::<LittleEndian>(u32::try_from(index + 1).map_err(|_| GMAError::FormatError)?)
				.map_err(|error| GMAError::io("write archive", &self.path, error))?;
			f.write_nt_string(&entry.archive_path).map_err(|error| GMAError::io("write archive", &self.path, error))?;
			positions.push(f.stream_position().map_err(|error| GMAError::io("seek archive", &self.path, error))?);
			f.write_i64::<LittleEndian>(0).map_err(|error| GMAError::io("write archive", &self.path, error))?;
			f.write_u32::<LittleEndian>(0).map_err(|error| GMAError::io("write archive", &self.path, error))?;
		}
		f.write_u32::<LittleEndian>(0).map_err(|error| GMAError::io("write archive", &self.path, error))?;
		for (index, entry) in entries.iter().enumerate() {
			check_cancelled(&transaction)?;
			let mut source = File::open(&entry.source_path).map_err(|error| GMAError::io("read source file", &entry.source_path, error))?;
			let before = source.metadata().map_err(|error| GMAError::io("read source metadata", &entry.source_path, error))?;
			let (size, crc) = read_contents(&mut source, &mut f, &entry.source_path, &self.path, &transaction)?;
			let after = source.metadata().map_err(|error| GMAError::io("read source metadata", &entry.source_path, error))?;
			if size != entry.size || size != after.len() || before.modified().ok() != after.modified().ok() {
				return Err(GMAError::SourceChanged(entry.source_path.clone()));
			}
			let end = f.stream_position().map_err(|error| GMAError::io("seek archive", &self.path, error))?;
			f.seek(std::io::SeekFrom::Start(positions[index])).map_err(|error| GMAError::io("seek archive", &self.path, error))?;
			f.write_i64::<LittleEndian>(i64::try_from(size).map_err(|_| GMAError::FormatError)?)
				.map_err(|error| GMAError::io("write archive", &self.path, error))?;
			f.write_u32::<LittleEndian>(crc).map_err(|error| GMAError::io("write archive", &self.path, error))?;
			f.seek(std::io::SeekFrom::Start(end)).map_err(|error| GMAError::io("seek archive", &self.path, error))?;
			transaction.progress((index + 1) as f64 / entries.len() as f64);
		}
		f.flush().map_err(|error| GMAError::io("flush archive", &self.path, error))?;
		let mut archive = File::open(&self.path).map_err(|error| GMAError::io("read archive checksum", &self.path, error))?;
		let (_, crc) = read_contents(&mut archive, &mut std::io::sink(), &self.path, &self.path, &transaction)?;
		f.write_u32::<LittleEndian>(crc).map_err(|error| GMAError::io("write archive footer", &self.path, error))?;

		check_cancelled(&transaction)?;
		f.flush().map_err(|error| GMAError::io("flush archive", &self.path, error))?;
		check_cancelled(&transaction)?;

		Ok(())
	}
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageRequest {
	content_path: PathBuf,
	destination: PathBuf,
	title: String,
	addon_type: String,
	tags: Vec<String>,
}

#[tauri::command]
pub fn package_addon(request: PackageRequest) -> u32 {
	let transaction = crate::transactions::new_publish();
	let id = transaction.id;
	transaction.context(serde_json::json!({ "kind": "package", "sourcePath": request.content_path, "destination": request.destination }));
	std::thread::spawn(move || {
		let ignore = app_data!().settings.read().ignore_globs.clone();
		match package_archive(request, ignore, &transaction) {
			Ok(path) => transaction.finished(serde_json::json!({ "path": path })),
			Err(GMAError::Cancelled) => transaction.cancelled(),
			Err(error) => transaction.error(error.to_string(), turbonone!()),
		}
	});
	id
}

fn package_archive(request: PackageRequest, ignore: Vec<String>, transaction: &Transaction) -> Result<PathBuf, GMAError> {
	check_cancelled(transaction)?;
	let PackageRequest { content_path, destination, title, addon_type, tags } = request;
	if title.trim().is_empty() || title.contains('\0')
		|| !["ServerContent", "gamemode", "map", "weapon", "vehicle", "npc", "tool", "effects", "model", "entity"].contains(&addon_type.as_str())
		|| tags.len() > 3 || tags.iter().any(|tag| !["fun", "roleplay", "scenic", "movie", "realism", "cartoon", "water", "comic", "build"].contains(&tag.as_str()))
	{
		return Err(GMAError::InvalidPackageMetadata);
	}
	if !destination.is_absolute() || !destination.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("gma")) {
		return Err(GMAError::InvalidPackageDestination);
	}
	if !destination.file_name().and_then(|name| name.to_str()).is_some_and(super::filename::valid_gma_file_name) {
		return Err(GMAError::InvalidPackageFileName);
	}
	let parent = destination.parent().ok_or(GMAError::InvalidPackageDestination)?;
	let parent = dunce::canonicalize(parent).map_err(|error| GMAError::io("open package destination", parent, error))?;
	let source = dunce::canonicalize(&content_path).map_err(|error| GMAError::io("open addon folder", &content_path, error))?;
	if parent.starts_with(&source) {
		return Err(GMAError::PackageInsideSource);
	}
	let destination = parent.join(destination.file_name().ok_or(GMAError::InvalidPackageDestination)?);
	let manifest = ContentManifest::build(&source, &ignore, || transaction.aborted())?;
	let stage = tempfile::Builder::new().prefix(".nwmpublisher-package-").tempdir_in(&parent)
		.map_err(|error| GMAError::io("create package staging", &parent, error))?;
	let stage_path = stage.path().to_owned();
	let result = (|| {
		if !transaction.begin_packing() { return Err(GMAError::Cancelled); }
		transaction.status("PUBLISH_PACKING");
		let gma = GMAFile {
			path: stage_path.join("addon.gma"),
			size: 0,
			id: None,
			metadata: Some(GMAMetadata::Standard { title, addon_type, tags, ignore }),
			entries: None,
			pointers: Default::default(),
			version: 3,
			extracted_name: String::new(),
			modified: None,
			membuffer: None,
			spool: None,
		};
		gma.create(manifest, transaction.clone())?;
		std::fs::OpenOptions::new().write(true).open(&gma.path).and_then(|file| file.sync_all())
			.map_err(|error| GMAError::io("sync package", &gma.path, error))?;
		if !transaction.begin_commit() { return Err(GMAError::Cancelled); }
		std::fs::rename(&gma.path, &destination).map_err(|error| GMAError::io("save package", &destination, error))?;
		Ok(destination)
	})();
	if let Err(error) = stage.close() {
		transaction.warning(GMAError::io("clean package staging", &stage_path, error).to_string());
	}
	result
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::gma::ExtractGMAMut;
	use std::fs;
	use std::io;

	fn package_request(root: &Path) -> PackageRequest {
		PackageRequest {
			content_path: root.join("source"),
			destination: root.join("saved.gma"),
			title: "Local addon".into(),
			addon_type: "tool".into(),
			tags: vec!["build".into()],
		}
	}

	#[test]
	fn local_package_replaces_destination_with_valid_archive_and_cleans_staging() {
		let root = tempfile::tempdir().unwrap();
		let root = root.path().canonicalize().unwrap();
		fs::create_dir_all(root.join("source/lua")).unwrap();
		fs::write(root.join("source/lua/main.lua"), b"print('local')").unwrap();
		fs::write(root.join("source/lua/skip.lua"), b"ignored").unwrap();
		fs::write(root.join("saved.gma"), b"previous archive").unwrap();
		let transaction = crate::transactions::new_publish();
		let path = package_archive(package_request(&root), vec!["lua/skip.lua".into()], &transaction).unwrap();
		transaction.finished(());
		let mut archive = GMAFile::open(&path).unwrap();
		let extraction = transaction!();
		archive.extract(crate::gma::ExtractDestination::Directory(root.join("extracted")), &extraction, false, true, &Default::default()).unwrap();
		extraction.cancel();
		assert_eq!(fs::read(root.join("extracted/lua/main.lua")).unwrap(), b"print('local')");
		assert!(!root.join("extracted/lua/skip.lua").exists());
		assert_eq!(archive.metadata.as_ref().unwrap().title(), "Local addon");
		assert_eq!(archive.metadata.as_ref().unwrap().addon_type(), Some("tool"));
		assert!(fs::read_dir(&root).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".nwmpublisher-package-")));
	}

	#[test]
	fn local_package_failure_and_cancellation_preserve_existing_files() {
		let root = tempfile::tempdir().unwrap();
		let root = root.path().canonicalize().unwrap();
		fs::create_dir_all(root.join("source/lua")).unwrap();
		fs::write(root.join("source/lua/main.lua"), b"print('local')").unwrap();
		fs::write(root.join("source/invalid.exe"), b"invalid").unwrap();
		fs::write(root.join("saved.gma"), b"keep").unwrap();
		let transaction = crate::transactions::new_publish();
		assert!(matches!(package_archive(package_request(&root), vec![], &transaction), Err(GMAError::NotWhitelisted(_))));
		transaction.error("test", ());
		assert_eq!(fs::read(root.join("saved.gma")).unwrap(), b"keep");
		fs::remove_file(root.join("source/invalid.exe")).unwrap();
		let transaction = crate::transactions::new_publish();
		transaction.cancel();
		assert!(matches!(package_archive(package_request(&root), vec![], &transaction), Err(GMAError::Cancelled)));
		transaction.cancelled();
		assert_eq!(fs::read(root.join("saved.gma")).unwrap(), b"keep");
		fs::remove_file(root.join("saved.gma")).unwrap();
		fs::create_dir(root.join("saved.gma")).unwrap();
		fs::write(root.join("saved.gma/keep"), b"keep").unwrap();
		let transaction = crate::transactions::new_publish();
		assert!(matches!(package_archive(package_request(&root), vec![], &transaction), Err(GMAError::IOError(_))));
		transaction.error("test", ());
		assert_eq!(fs::read(root.join("saved.gma/keep")).unwrap(), b"keep");
		assert!(fs::read_dir(&root).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".nwmpublisher-package-")));
	}

	#[test]
	fn local_package_rejects_invalid_metadata_and_output_inside_source() {
		let root = tempfile::tempdir().unwrap();
		let root = root.path().canonicalize().unwrap();
		fs::create_dir(root.join("source")).unwrap();
		let mut request = package_request(&root);
		request.title = "invalid\0title".into();
		let transaction = crate::transactions::new_publish();
		assert!(matches!(package_archive(request, vec![], &transaction), Err(GMAError::InvalidPackageMetadata)));
		transaction.error("test", ());
		let mut request = package_request(&root);
		request.destination = root.join("source/output.gma");
		let transaction = crate::transactions::new_publish();
		assert!(matches!(package_archive(request, vec![], &transaction), Err(GMAError::PackageInsideSource)));
		transaction.error("test", ());
		assert_eq!(fs::read_dir(root.join("source")).unwrap().count(), 0);
		let mut request = package_request(&root);
		request.destination = root.join("output.lua");
		let transaction = crate::transactions::new_publish();
		assert!(matches!(package_archive(request, vec![], &transaction), Err(GMAError::InvalidPackageDestination)));
		transaction.error("test", ());
	}

	#[test]
	fn local_package_rejects_nonportable_names_without_touching_files() {
		let root = tempfile::tempdir().unwrap();
		fs::create_dir(root.path().join("source")).unwrap();
		fs::write(root.path().join("publishedaddon.gma"), b"keep").unwrap();
		for name in ["CON.gma", "nul.tar.gma", "COM¹.gma", "addon?.gma", "addon. .gma", " addon.gma"] {
			let mut request = package_request(root.path());
			request.destination = root.path().join(name);
			let transaction = crate::transactions::new_publish();
			assert!(matches!(package_archive(request, vec![], &transaction), Err(GMAError::InvalidPackageFileName)), "{name}");
			transaction.error("test", ());
		}
		assert_eq!(fs::read(root.path().join("publishedaddon.gma")).unwrap(), b"keep");
		assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
	}

	fn independent_crc(bytes: &[u8]) -> u32 {
		let mut crc = u32::MAX;
		for byte in bytes {
			crc ^= u32::from(*byte);
			for _ in 0..8 { crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1)); }
		}
		!crc
	}

	#[test]
	fn cancellation_interrupts_large_file_reads_and_archive_writes() {
		struct CancelAfterChunk {
			transaction: Transaction,
			bytes: usize,
		}
		impl Read for CancelAfterChunk {
			fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
				buffer.fill(1);
				self.bytes += buffer.len();
				if self.bytes >= PACK_CHUNK_SIZE {
					self.transaction.cancel();
				}
				Ok(buffer.len())
			}
		}
		impl Write for CancelAfterChunk {
			fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
				self.bytes += buffer.len();
				self.transaction.cancel();
				Ok(buffer.len())
			}
			fn flush(&mut self) -> io::Result<()> {
				Ok(())
			}
		}
		let path = Path::new("test.gma");
		let transaction = crate::transactions::new_publish();
		let mut reader = CancelAfterChunk {
			transaction: transaction.clone(),
			bytes: 0,
		};
		assert!(matches!(read_contents(&mut reader, &mut io::sink(), path, path, &transaction), Err(GMAError::Cancelled)));
		assert_eq!(reader.bytes, PACK_CHUNK_SIZE);
		transaction.cancelled();

		let transaction = crate::transactions::new_publish();
		let mut writer = CancelAfterChunk {
			transaction: transaction.clone(),
			bytes: 0,
		};
		assert!(matches!(
			read_contents(&mut &vec![1; PACK_CHUNK_SIZE * 3][..], &mut writer, path, path, &transaction),
			Err(GMAError::Cancelled)
		));
		assert_eq!(writer.bytes, PACK_CHUNK_SIZE);
		transaction.cancelled();
	}

	#[test]
	fn packing_reports_io_failures_and_round_trips_files() {
		let root = std::env::temp_dir().canonicalize().unwrap().join(format!("nwmpublisher-packing-{}", std::process::id()));
		fs::create_dir(&root).unwrap();
		let mut gma = GMAFile {
			path: root.join("test.gma"),
			size: 0,
			id: None,
			metadata: Some(GMAMetadata::Standard {
				title: "Test".into(),
				addon_type: "tool".into(),
				tags: vec![],
				ignore: vec![],
			}),
			entries: None,
			pointers: Default::default(),
			version: 3,
			extracted_name: "test".into(),
			modified: None,
			membuffer: None,
			spool: None,
		};
		let source = root.join("source");
		let error = ContentManifest::build(&source, &[], || false).unwrap_err();
		assert!(error.to_string().contains("read content metadata"));
		assert!(error.to_string().contains("source"));
		assert!(!gma.path.exists());

		fs::create_dir_all(source.join("LUA")).unwrap();
		let file = source.join("LUA/Test.LUA");
		fs::write(&file, b"print('test')").unwrap();
		let large_file = vec![42; PACK_CHUNK_SIZE * 3 + 5];
		fs::write(source.join("LUA/large.lua"), &large_file).unwrap();
		fs::write(source.join("LUA/skip.lua"), b"ignored").unwrap();
		fs::write(source.join("README.md"), b"default ignored").unwrap();
		fs::create_dir_all(source.join(".git/lua")).unwrap();
		fs::write(source.join(".git/lua/ignored.lua"), b"default ignored").unwrap();
		let ignore = vec!["lua/skip.lua".to_owned()];
		let manifest = ContentManifest::build(&source, &ignore, || false).unwrap();
		let cancelled = crate::transactions::new_publish();
		cancelled.cancel();
		assert!(matches!(gma.create(manifest, cancelled.clone()), Err(GMAError::Cancelled)));
		assert!(!gma.path.exists());
		cancelled.cancelled();
		#[cfg(target_os = "windows")]
		{
			use std::os::windows::fs::OpenOptionsExt;
			let manifest = ContentManifest::build(&source, &ignore, || false).unwrap();
			let lock = fs::OpenOptions::new().read(true).share_mode(0).open(&file).unwrap();
			let transaction = transaction!();
			let error = gma.create(manifest, transaction.clone()).unwrap_err();
			assert!(error.to_string().contains("read source file"));
			assert!(error.to_string().contains("Test.LUA"));
			transaction.cancel();
			drop(lock);
		}
		let (preview, preview_size) = ContentManifest::build(&source, &ignore, || false).unwrap().into_preview();
		let manifest = ContentManifest::build(&source, &ignore, || false).unwrap();
		let transaction = transaction!();
		gma.create(manifest, transaction.clone()).unwrap();
		let archive = fs::read(&gma.path).unwrap();
		assert_eq!(independent_crc(b"123456789"), 0xcbf43926);
		assert_eq!(u32::from_le_bytes(archive[archive.len() - 4..].try_into().unwrap()), independent_crc(&archive[..archive.len() - 4]));
		transaction.cancel();
		let mut packed = GMAFile::open(&gma.path).unwrap();
		let transaction = transaction!();
		let destination = root.join("extracted");
		packed
			.extract(crate::gma::ExtractDestination::Directory(destination.clone()), &transaction, false, true, &Default::default())
			.unwrap();
		let entries = packed.entries.as_ref().unwrap();
		assert_eq!(entries.len(), preview.len());
		assert_eq!(entries.values().map(|entry| entry.size).sum::<u64>(), preview_size);
		for entry in preview {
			assert_eq!(entries[&entry.path].size, entry.size);
		}
		assert_eq!(fs::read(destination.join("lua/test.lua")).unwrap(), b"print('test')");
		assert_eq!(fs::read(destination.join("lua/large.lua")).unwrap(), large_file);
		assert!(!destination.join("lua/skip.lua").exists());

		gma.path = root.join("missing/test.gma");
		let manifest = ContentManifest::build(&source, &ignore, || false).unwrap();
		let transaction = transaction!();
		let error = gma.create(manifest, transaction.clone()).unwrap_err();
		assert!(error.to_string().contains("create archive"));
		assert!(error.to_string().contains("test.gma"));
		transaction.cancel();
		fs::remove_dir_all(root).unwrap();
	}
}
