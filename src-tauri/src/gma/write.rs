use byteorder::{LittleEndian, WriteBytesExt};
use lazy_static::lazy_static;
use rayon::ThreadPool;
use std::{
	collections::BTreeMap,
	fs::File,
	io::{BufWriter, Read, Seek, Write},
	path::Path,
	time::SystemTime,
};

use path_slash::PathExt;
use walkdir::WalkDir;

use crate::{transactions::Transaction, GMAFile, NTStringWriter};

use super::{whitelist, GMAError, GMAMetadata};

use super::GMA_HEADER;

lazy_static! {
	static ref THREAD_POOL: ThreadPool = thread_pool!();
}

impl NTStringWriter for BufWriter<File> {}

const PACK_CHUNK_SIZE: usize = 64 * 1024;

fn check_cancelled(transaction: &Transaction) -> Result<(), GMAError> {
	if transaction.aborted() {
		Err(GMAError::Cancelled)
	} else {
		Ok(())
	}
}

fn read_contents(reader: &mut impl Read, path: &Path, transaction: &Transaction) -> Result<(Box<[u8]>, u32), GMAError> {
	let mut contents = Vec::new();
	let mut crc32 = crc32fast::Hasher::new();
	loop {
		check_cancelled(transaction)?;
		let start = contents.len();
		let read = reader
			.by_ref()
			.take(PACK_CHUNK_SIZE as u64)
			.read_to_end(&mut contents)
			.map_err(|error| GMAError::io("read source file", path, error))?;
		check_cancelled(transaction)?;
		if read == 0 {
			break;
		}
		crc32.update(&contents[start..]);
	}
	Ok((contents.into_boxed_slice(), crc32.finalize()))
}

fn write_contents(writer: &mut impl Write, contents: &[u8], path: &Path, transaction: &Transaction) -> Result<(), GMAError> {
	for chunk in contents.chunks(PACK_CHUNK_SIZE) {
		check_cancelled(transaction)?;
		writer.write_all(chunk).map_err(|error| GMAError::io("write archive", path, error))?;
	}
	check_cancelled(transaction)
}

impl GMAFile {
	pub fn write(&self) -> Result<BufWriter<File>, GMAError> {
		Ok(BufWriter::new(
			File::create(&self.path).map_err(|error| GMAError::io("create archive", &self.path, error))?,
		))
	}

	pub fn create<P: AsRef<Path>>(&self, src_path: P, transaction: Transaction) -> Result<(), GMAError> {
		// Wait for all packing workers before the caller cleans up and acknowledges cancellation.
		THREAD_POOL.in_place_scope(|scope| self.create_scoped(src_path.as_ref(), transaction, scope))
	}

	fn create_scoped(&self, src_path: &Path, transaction: Transaction, scope: &rayon::Scope<'_>) -> Result<(), GMAError> {
		check_cancelled(&transaction)?;
		let mut f = self.write()?;

		let metadata = self.metadata.as_ref().expect("Expected metadata to be set");
		let ignore = metadata.ignore().map(|ignore| ignore.to_vec().into_boxed_slice());

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

		// file list
		let mut file_list: BTreeMap<String, (usize, u64, Box<[u8]>)> = BTreeMap::new();
		let (rx, total) = {
			let (tx, rx) = crossbeam::channel::unbounded();

			let root_path_strip_len = src_path.to_string_lossy().len();

			let mut total = 0.;
			for entry in WalkDir::new(src_path).follow_links(false) {
				check_cancelled(&transaction)?;
				let entry = entry.map_err(|error| {
					let path = error.path().unwrap_or(src_path).to_owned();
					GMAError::io("read content directory", &path, error.into())
				})?;
				if !entry.file_type().is_file() {
					continue;
				}
				let path = entry.into_path();
				let relative_path = path.to_slash_lossy()[root_path_strip_len..].trim_matches('/').to_lowercase();
				if !whitelist::check(&relative_path) {
					transaction.data(("ERR_WHITELIST", relative_path));
					continue;
				}
				if ignore.as_ref().is_some_and(|ignore| whitelist::is_ignored(&relative_path, ignore)) {
					continue;
				}

				file_list.insert(relative_path.clone(), (0, 0, Vec::new().into_boxed_slice()));

				let tx = tx.clone();
				let transaction = transaction.clone();
				scope.spawn(move |_| {
					let result = (|| {
						check_cancelled(&transaction)?;
						let mut file = File::open(&path).map_err(|error| GMAError::io("read source file", &path, error))?;
						let (contents, crc32) = read_contents(&mut file, &path, &transaction)?;
						Ok::<_, GMAError>((relative_path.into_boxed_str(), contents, crc32))
					})();
					// The receiver is dropped when packing fails or is cancelled.
					let _ = tx.send(result);
				});

				total += 1.;
			}

			(rx, total)
		};

		let mut cursor = f.stream_position().map_err(|error| GMAError::io("seek archive", &self.path, error))?;
		for (i, (path, (idx, pos, _))) in file_list.iter_mut().enumerate() {
			check_cancelled(&transaction)?;
			*pos = cursor;
			*idx = i + 1;
			cursor += 4 + path.len() as u64 + 1 + 8 + 4; // index + path + null + size + crc32
		}

		let mut i_f: f64 = 0.;
		while let Ok(result) = rx.recv() {
			check_cancelled(&transaction)?;
			let (path, contents, crc32) = result?;
			let (i, cursor, read_contents) = file_list.get_mut(&*path).unwrap();

			*read_contents = contents;

			let contents = &**read_contents;

			f.seek(std::io::SeekFrom::Start(*cursor))
				.map_err(|error| GMAError::io("seek archive", &self.path, error))?;
			f.write_u32::<LittleEndian>(*i as u32)
				.map_err(|error| GMAError::io("write archive", &self.path, error))?;
			f.write_all(path.as_bytes())
				.map_err(|error| GMAError::io("write archive", &self.path, error))?;
			f.write_u8(0).map_err(|error| GMAError::io("write archive", &self.path, error))?;
			f.write_i64::<LittleEndian>(contents.len() as i64)
				.map_err(|error| GMAError::io("write archive", &self.path, error))?;
			f.write_u32::<LittleEndian>(crc32)
				.map_err(|error| GMAError::io("write archive", &self.path, error))?;

			i_f += 1.;
			transaction.progress(i_f / total);
		}

		check_cancelled(&transaction)?;
		f.seek(std::io::SeekFrom::Start(cursor))
			.map_err(|error| GMAError::io("seek archive", &self.path, error))?;
		f.write_u32::<LittleEndian>(0)
			.map_err(|error| GMAError::io("write archive", &self.path, error))?;

		for (_, (_, _, contents)) in file_list {
			write_contents(&mut f, &contents, &self.path, &transaction)?;
		}

		let written = f.buffer();

		let mut crc32 = crc32fast::Hasher::new();
		crc32.reset();
		crc32.update(written);
		let crc32 = crc32.finalize();

		f.write_u32::<LittleEndian>(crc32)
			.map_err(|error| GMAError::io("write archive", &self.path, error))?;

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
		assert!(matches!(read_contents(&mut reader, path, &transaction), Err(GMAError::Cancelled)));
		assert_eq!(reader.bytes, PACK_CHUNK_SIZE);
		transaction.cancelled();

		let transaction = crate::transactions::new_publish();
		let mut writer = CancelAfterChunk {
			transaction: transaction.clone(),
			bytes: 0,
		};
		assert!(matches!(
			write_contents(&mut writer, &vec![1; PACK_CHUNK_SIZE * 3], path, &transaction),
			Err(GMAError::Cancelled)
		));
		assert_eq!(writer.bytes, PACK_CHUNK_SIZE);
		transaction.cancelled();
	}

	#[test]
	fn packing_reports_io_failures_and_round_trips_files() {
		let root = std::env::temp_dir().join(format!("nwmpublisher-packing-{}", std::process::id()));
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
		};
		let transaction = transaction!();
		let source = root.join("source");
		let cancelled = crate::transactions::new_publish();
		cancelled.cancel();
		assert!(matches!(gma.create(&source, cancelled.clone()), Err(GMAError::Cancelled)));
		assert!(!gma.path.exists());
		cancelled.cancelled();
		let error = gma.create(&source, transaction.clone()).unwrap_err();
		assert!(error.to_string().contains("read content directory"));
		assert!(error.to_string().contains("source"));
		transaction.cancel();

		fs::create_dir_all(source.join("lua")).unwrap();
		let file = source.join("lua/test.lua");
		fs::write(&file, b"print('test')").unwrap();
		let large_file = vec![42; PACK_CHUNK_SIZE * 3 + 5];
		fs::write(source.join("lua/large.lua"), &large_file).unwrap();
		#[cfg(target_os = "windows")]
		{
			use std::os::windows::fs::OpenOptionsExt;
			let lock = fs::OpenOptions::new().read(true).share_mode(0).open(&file).unwrap();
			let transaction = transaction!();
			let error = gma.create(&source, transaction.clone()).unwrap_err();
			assert!(error.to_string().contains("read source file"));
			assert!(error.to_string().contains("test.lua"));
			transaction.cancel();
			drop(lock);
		}
		let transaction = transaction!();
		gma.create(&source, transaction.clone()).unwrap();
		transaction.cancel();
		let mut packed = GMAFile::open(&gma.path).unwrap();
		let transaction = transaction!();
		let destination = root.join("extracted");
		packed
			.extract(crate::gma::ExtractDestination::Directory(destination.clone()), &transaction, false, true)
			.unwrap();
		assert_eq!(fs::read(destination.join("lua/test.lua")).unwrap(), b"print('test')");
		assert_eq!(fs::read(destination.join("lua/large.lua")).unwrap(), large_file);

		gma.path = root.join("missing/test.gma");
		let transaction = transaction!();
		let error = gma.create(&source, transaction.clone()).unwrap_err();
		assert!(error.to_string().contains("create archive"));
		assert!(error.to_string().contains("test.gma"));
		transaction.cancel();
		fs::remove_dir_all(root).unwrap();
	}
}
