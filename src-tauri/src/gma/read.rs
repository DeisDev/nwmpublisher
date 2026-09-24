use std::{
	collections::{HashMap, HashSet},
	fs::File,
	io::{BufReader, Cursor, SeekFrom},
};

use byteorder::{LittleEndian, ReadBytesExt};

use crate::{ArcBytes, NTStringReader};

use super::{GMAEntry, GMAError, GMAFile, GMAMetadata};

pub(super) fn is_unsafe_entry_path(path: &str) -> bool {
	if path.is_empty() || path.len() > 4096 {
		return true;
	}
	if path.bytes().any(|b| b == 0 || b == b':' || b == b'\\') {
		return true;
	}
	if path.starts_with('/') {
		return true;
	}
	for segment in path.split('/') {
		let stem = segment.split('.').next().unwrap_or("").to_uppercase();
		let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
			|| ["COM", "LPT"].iter().any(|prefix| stem.strip_prefix(*prefix).is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "\u{b9}" | "\u{b2}" | "\u{b3}")));
		if segment.is_empty() || segment == "." || segment == ".." || segment != segment.trim()
			|| segment.ends_with('.') || device || segment.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*')) {
			return true;
		}
	}
	false
}

pub enum GMAReader {
	MemBuffer(Cursor<ArcBytes>),
	Disk(BufReader<File>),
}
impl std::ops::Deref for GMAReader {
	type Target = dyn NTStringReader;

	fn deref(&self) -> &Self::Target {
		match self {
			Self::MemBuffer(buf) => buf,
			Self::Disk(buf) => buf,
		}
	}
}
impl std::ops::DerefMut for GMAReader {
	fn deref_mut(&mut self) -> &mut Self::Target {
		match self {
			Self::MemBuffer(buf) => buf,
			Self::Disk(buf) => buf,
		}
	}
}
impl NTStringReader for Cursor<ArcBytes> {}
impl NTStringReader for BufReader<File> {}

impl GMAFile {
	pub fn read(&self) -> Result<GMAReader, GMAError> {
		if let Some(ref membuffer) = self.membuffer {
			Ok(GMAReader::MemBuffer(Cursor::new(membuffer.clone())))
		} else if let Some(spool) = &self.spool {
			Ok(GMAReader::Disk(BufReader::new(spool.reopen().map_err(|error| GMAError::io("reopen decompressed archive", spool.path(), error))?)))
		} else {
			Ok(GMAReader::Disk(BufReader::new(
				File::open(&self.path).map_err(|error| GMAError::io("open archive", &self.path, error))?,
			)))
		}
	}

	pub fn metadata(&mut self) -> Result<Option<GMAReader>, GMAError> {
		main_thread_forbidden!();

		if self.metadata.is_some() {
			Ok(None)
		} else {
			let mut handle = self.read()?;
			handle
				.seek(SeekFrom::Start(self.pointers.metadata))
				.map_err(|error| GMAError::io("seek archive", &self.path, error))?;

			handle
				.read_u64::<LittleEndian>()
				.map_err(|error| GMAError::io("read metadata", &self.path, error))?; // steamid [unused]
			handle
				.read_u64::<LittleEndian>()
				.map_err(|error| GMAError::io("read metadata", &self.path, error))?; // timestamp

			if self.version > 1 {
				// required content [unused]
				handle
					.skip_nt_string()
					.map_err(|error| GMAError::io("read metadata", &self.path, error))?;
			}

			let embedded_title = handle
				.read_nt_string()
				.map_err(|error| GMAError::io("read metadata", &self.path, error))?;
			let embedded_description = handle
				.read_nt_string()
				.map_err(|error| GMAError::io("read metadata", &self.path, error))?;

			let metadata = Some(match serde_json::de::from_str::<GMAMetadata>(&embedded_description) {
				Ok(mut metadata) => {
					match &mut metadata {
						GMAMetadata::Standard { title, .. } => *title = embedded_title,
						GMAMetadata::Legacy { title, description } => {
							*title = embedded_title;
							*description = embedded_description;
						}
					}
					metadata
				}
				Err(_) => GMAMetadata::Legacy {
					title: embedded_title,
					description: embedded_description,
				},
			});

			handle
				.skip_nt_string()
				.map_err(|error| GMAError::io("read metadata", &self.path, error))?; // author [unused]
			handle
				.read_i32::<LittleEndian>()
				.map_err(|error| GMAError::io("read metadata", &self.path, error))?; // addon version [unused]

			self.pointers.entries_list = handle
				.seek(SeekFrom::Current(0))
				.map_err(|error| GMAError::io("seek archive", &self.path, error))?;

			self.metadata = metadata;
			self.compute_extracted_name();

			Ok(Some(handle))
		}
	}

	// https://steamcommunity.com/sharedfiles/filedetails/?id=1727993520

	pub fn entries(&mut self) -> Result<Option<GMAReader>, GMAError> {
		main_thread_forbidden!();

		if self.entries.is_some() {
			Ok(None)
		} else {
			let mut handle = match self.metadata()? {
				Some(handle) => handle,
				None => self.read()?,
			};
			handle
				.seek(SeekFrom::Start(self.pointers.entries_list))
				.map_err(|error| GMAError::io("seek archive", &self.path, error))?;

			let mut entries = HashMap::new();
			let mut targets = HashSet::new();
			let mut directories = HashSet::new();
			let mut table_bytes = 0usize;
			let mut entry_cursor: u64 = 0;

			while handle
				.read_u32::<LittleEndian>()
				.map_err(|error| GMAError::io("read entry table", &self.path, error))?
				!= 0
			{
				let path = handle
					.read_nt_string()
					.map_err(|error| GMAError::io("read entry table", &self.path, error))?;
				let size = handle
					.read_i64::<LittleEndian>()
					.map_err(|error| GMAError::io("read entry table", &self.path, error))?;
				let size = u64::try_from(size).map_err(|_| GMAError::MetadataError(format!("negative entry size: {}", path)))?;
				let crc = handle
					.read_u32::<LittleEndian>()
					.map_err(|error| GMAError::io("read entry table", &self.path, error))?;

				let next_cursor = match entry_cursor.checked_add(size) {
					None => return Err(GMAError::FormatError),
					Some(next_cursor) => next_cursor,
				};

				table_bytes = table_bytes.checked_add(path.len() + 17).ok_or(GMAError::FormatError)?;
				if entries.len() >= 100_000 || table_bytes > 32 * 1024 * 1024 {
					return Err(GMAError::LimitExceeded("entry table".into()));
				}
				if is_unsafe_entry_path(&path) { return Err(GMAError::UnsafeEntry(path)); }
				let target = path.to_uppercase();
				if !targets.insert(target.clone()) || directories.contains(&target) { return Err(GMAError::DuplicateEntry(path)); }
				let mut parent = target.as_str();
				while let Some((directory, _)) = parent.rsplit_once('/') {
					if targets.contains(directory) { return Err(GMAError::DuplicateEntry(path)); }
					directories.insert(directory.to_owned());
					parent = directory;
				}

				let entry = GMAEntry {
					path: path.clone(),
					size,
					crc,
					index: entry_cursor,
				};

				entry_cursor = next_cursor;

				entries.insert(path, entry);
			}

			self.pointers.entries = handle
				.seek(SeekFrom::Current(0))
				.map_err(|error| GMAError::io("seek archive", &self.path, error))?;

			let end = self.pointers.entries.checked_add(entry_cursor).ok_or(GMAError::FormatError)?;
			let actual_size = crate::stream_len(&mut *handle).map_err(|error| GMAError::io("measure archive", &self.path, error))?;
			if end > actual_size || (actual_size != end && actual_size.checked_sub(end) != Some(4)) {
				return Err(GMAError::MetadataError("entry ranges do not match archive length".into()));
			}
			self.size = actual_size;
			self.entries = Some(entries);
			Ok(Some(handle))
		}
	}
	pub(super) fn validate_payloads(&self, transaction: &crate::Transaction) -> Result<(), GMAError> {
		let entries = self.entries.as_ref().ok_or(GMAError::FormatError)?;
		let mut reader = self.read()?;
		let mut buffer = [0; 64 * 1024];
		let mut end = self.pointers.entries;
		for entry in entries.values() {
			let offset = self.pointers.entries.checked_add(entry.index).ok_or(GMAError::FormatError)?;
			end = end.max(offset.checked_add(entry.size).ok_or(GMAError::FormatError)?);
			reader.seek(SeekFrom::Start(offset)).map_err(|error| GMAError::io("seek archive entry", &self.path, error))?;
			let mut remaining = entry.size;
			let mut hash = crc32fast::Hasher::new();
			while remaining > 0 {
				if transaction.aborted() { return Err(GMAError::Cancelled); }
				let count = remaining.min(buffer.len() as u64) as usize;
				reader.read_exact(&mut buffer[..count]).map_err(|error| GMAError::io("read archive entry", &self.path, error))?;
				hash.update(&buffer[..count]);
				remaining -= count as u64;
			}
			if entry.crc != 0 && entry.crc != hash.finalize() { return Err(GMAError::Checksum(entry.path.clone())); }
		}
		let actual_size = crate::stream_len(&mut *reader).map_err(|error| GMAError::io("measure archive", &self.path, error))?;
		if actual_size == end { return Ok(()); } // Legacy archives may omit the footer.
		if actual_size.checked_sub(end) != Some(4) { return Err(GMAError::FormatError); }
		reader.seek(SeekFrom::Start(end)).map_err(|error| GMAError::io("seek archive footer", &self.path, error))?;
		let footer = reader.read_u32::<LittleEndian>().map_err(|error| GMAError::io("read archive footer", &self.path, error))?;
		if footer == 0 { return Ok(()); } // Facepunch gmad -nocrc explicitly writes zero CRCs.
		reader.seek(SeekFrom::Start(0)).map_err(|error| GMAError::io("seek archive", &self.path, error))?;
		let mut hash = crc32fast::Hasher::new();
		while end > 0 {
			if transaction.aborted() { return Err(GMAError::Cancelled); }
			let count = end.min(buffer.len() as u64) as usize;
			reader.read_exact(&mut buffer[..count]).map_err(|error| GMAError::io("read archive checksum", &self.path, error))?;
			hash.update(&buffer[..count]);
			end -= count as u64;
		}
		if hash.finalize() != footer { return Err(GMAError::Checksum(self.path.display().to_string())); }
		Ok(())
	}

}

#[cfg(test)]
mod tests {
	use super::*;
	use byteorder::WriteBytesExt;
	use crate::NTStringWriter;

	fn archive(entries: &[(&str, i64, u32)], payload: &[u8], footer: Option<u32>) -> GMAFile {
		let mut bytes = b"GMAD\x03".to_vec();
		bytes.extend_from_slice(&[0; 17]);
		for text in ["Test", "{}", "Author"] { bytes.write_nt_string(text).unwrap(); }
		bytes.write_i32::<LittleEndian>(1).unwrap();
		for (index, (path, size, crc)) in entries.iter().enumerate() {
			bytes.write_u32::<LittleEndian>((index + 1) as u32).unwrap();
			bytes.write_nt_string(path).unwrap();
			bytes.write_i64::<LittleEndian>(*size).unwrap();
			bytes.write_u32::<LittleEndian>(*crc).unwrap();
		}
		bytes.write_u32::<LittleEndian>(0).unwrap();
		bytes.extend_from_slice(payload);
		if let Some(footer) = footer { bytes.write_u32::<LittleEndian>(footer).unwrap(); }
		GMAFile::read_header(GMAReader::MemBuffer(Cursor::new(bytes.into())), "fixture.gma").unwrap()
	}

	#[test]
	fn preflight_rejects_negative_ranges_duplicates_and_aliases() {
		for entries in [
			vec![("lua/a.lua", -1, 0)], vec![("lua/a.lua", 8, 0)],
			vec![("lua/a.lua", 0, 0), ("LUA/A.LUA", 0, 0)],
			vec![("lua", 0, 0), ("lua/a.lua", 0, 0)],
			vec![("lua/a.lua", 0, 0), ("lua", 0, 0)],
			vec![("lua/NUL.txt", 0, 0)], vec![("lua/a.lua.", 0, 0)], vec![("../outside", 0, 0)],
			vec![("lua/COM\u{b9}.txt", 0, 0)], vec![("lua/LPT\u{b2}.txt", 0, 0)],
		] { assert!(archive(&entries, &[], None).entries().is_err(), "{entries:?}"); }
	}

	#[test]
	fn legacy_absent_and_zero_checksums_are_accepted_but_corruption_is_not() {
		for footer in [None, Some(0)] {
			let mut gma = archive(&[("lua/a.lua", 4, 0)], b"test", footer);
			gma.entries().unwrap();
			let transaction = crate::transactions::new_extraction();
			gma.validate_payloads(&transaction).unwrap();
			transaction.finished(());
		}
		for (crc, footer) in [(1, None), (0, Some(1))] {
			let mut gma = archive(&[("lua/a.lua", 4, crc)], b"test", footer);
			gma.entries().unwrap();
			let transaction = crate::transactions::new_extraction();
			assert!(matches!(gma.validate_payloads(&transaction), Err(GMAError::Checksum(_))));
			transaction.finished(());
		}
	}

	#[test]
	fn rejects_absolute_unix() {
		assert!(is_unsafe_entry_path("/etc/passwd"));
		assert!(is_unsafe_entry_path("/"));
	}

	#[test]
	fn rejects_absolute_windows_root() {
		assert!(is_unsafe_entry_path(
			"\\Program Files (x86)\\Steam\\steamapps\\common\\GarrysMod\\garrysmod\\lua\\bin\\evil.dll"
		));
		assert!(is_unsafe_entry_path("\\evil.dll"));
	}

	#[test]
	fn rejects_embedded_backslash() {
		assert!(is_unsafe_entry_path(
			"Program Files (x86)\\Steam\\steamapps\\common\\GarrysMod\\garrysmod\\lua\\bin\\haha.dll"
		));
		assert!(is_unsafe_entry_path("lua\\autorun\\evil.lua"));
		assert!(is_unsafe_entry_path("foo\\bar"));
	}

	#[test]
	fn rejects_segment_whitespace() {
		assert!(is_unsafe_entry_path(" Files (x86)/Steam/foo"));
		assert!(is_unsafe_entry_path("lua/ autorun/foo.lua"));
		assert!(is_unsafe_entry_path("lua/autorun /foo.lua"));
		assert!(is_unsafe_entry_path("\tfoo/bar"));
	}

	#[test]
	fn rejects_drive_letter() {
		assert!(is_unsafe_entry_path("C:\\evil.dll"));
		assert!(is_unsafe_entry_path("c:evil.dll"));
		assert!(is_unsafe_entry_path("file.txt:stream"));
	}

	#[test]
	fn rejects_unc_and_long_paths() {
		assert!(is_unsafe_entry_path("\\\\server\\share\\evil"));
		assert!(is_unsafe_entry_path("\\\\?\\C:\\evil"));
	}

	#[test]
	fn rejects_parent_traversal() {
		assert!(is_unsafe_entry_path("../etc/passwd"));
		assert!(is_unsafe_entry_path("..\\evil.dll"));
		assert!(is_unsafe_entry_path("foo/../../bar"));
		assert!(is_unsafe_entry_path("foo\\..\\bar"));
		assert!(is_unsafe_entry_path(".."));
	}

	#[test]
	fn rejects_current_dir_segments() {
		assert!(is_unsafe_entry_path("./foo"));
		assert!(is_unsafe_entry_path("foo/./bar"));
		assert!(is_unsafe_entry_path("."));
	}

	#[test]
	fn rejects_empty_or_null() {
		assert!(is_unsafe_entry_path(""));
		assert!(is_unsafe_entry_path("foo\0bar"));
		assert!(is_unsafe_entry_path("foo//bar"));
	}

	#[test]
	fn accepts_normal_entries() {
		assert!(!is_unsafe_entry_path("lua/autorun/foo.lua"));
		assert!(!is_unsafe_entry_path("materials/models/foo.vmt"));
		assert!(!is_unsafe_entry_path("addon.json"));
		assert!(!is_unsafe_entry_path("foo..bar/baz"));
	}
}
