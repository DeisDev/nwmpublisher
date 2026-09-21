use std::{
	collections::HashMap,
	fs::File,
	io::{BufReader, Cursor, SeekFrom},
};

use byteorder::{LittleEndian, ReadBytesExt};

use crate::{ArcBytes, NTStringReader};

use super::{GMAEntry, GMAError, GMAFile, GMAMetadata};

fn is_unsafe_entry_path(path: &str) -> bool {
	if path.is_empty() {
		return true;
	}
	if path.bytes().any(|b| b == 0 || b == b':' || b == b'\\') {
		return true;
	}
	if path.starts_with('/') {
		return true;
	}
	for segment in path.split('/') {
		if segment.is_empty() || segment == "." || segment == ".." || segment != segment.trim() {
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
			let mut entry_cursor: u64 = 0;

			'read_entries: while handle
				.read_u32::<LittleEndian>()
				.map_err(|error| GMAError::io("read entry table", &self.path, error))?
				!= 0
			{
				let path = handle
					.read_nt_string()
					.map_err(|error| GMAError::io("read entry table", &self.path, error))?;
				let size = handle
					.read_i64::<LittleEndian>()
					.map_err(|error| GMAError::io("read entry table", &self.path, error))? as u64;
				let crc = handle
					.read_u32::<LittleEndian>()
					.map_err(|error| GMAError::io("read entry table", &self.path, error))?;

				let next_cursor = match entry_cursor.checked_add(size) {
					None => return Err(GMAError::FormatError),
					Some(next_cursor) => next_cursor,
				};

				if is_unsafe_entry_path(&path) {
					eprintln!("Illegal GMA entry: {}", path);
					entry_cursor = next_cursor;
					continue 'read_entries;
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

			self.entries = Some(entries);
			Ok(Some(handle))
		}
	}
}

#[cfg(test)]
mod tests {
	use super::is_unsafe_entry_path;

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
