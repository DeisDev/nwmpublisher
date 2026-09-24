use byteorder::{LittleEndian, WriteBytesExt};
use std::{
	fs::File,
	io::{BufWriter, Read, Seek, Write},
	path::Path,
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::gma::ExtractGMAMut;
	use std::fs;
	use std::io;

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
			.extract(crate::gma::ExtractDestination::Directory(destination.clone()), &transaction, false, true)
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
