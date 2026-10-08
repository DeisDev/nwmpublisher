use std::{io::{Read, SeekFrom}, path::{Path, PathBuf}, sync::Arc};
use serde::Serialize;

use super::{extract::ExtractGMAImmut, manifest::ContentManifest, output::Directory, ExtractDestination, GMAEntry, GMAError, GMAFile, GMAReader};
use parking_lot::Mutex;

lazy_static! {
	static ref PREVIEW_GMA: Mutex<Option<Arc<GMAFile>>> = Mutex::new(None);
}

#[tauri::command]
pub fn preview_gma(path: Option<PathBuf>) -> Result<Option<Vec<GMAEntry>>, GMAError> {
	if let Some(path) = path {
		let mut lock = PREVIEW_GMA.lock();

		let mut gma = GMAFile::open(path)?;
		gma.entries()?;
		*lock = Some(Arc::new(gma));

		let mut entries: Vec<GMAEntry> = lock.as_ref().unwrap().entries.as_ref().unwrap().values().cloned().collect();
		entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));

		Ok(Some(entries))
	} else {
		*PREVIEW_GMA.lock() = None;
		Ok(None)
	}
}

#[tauri::command]
pub fn extract_preview_entry(gma_path: PathBuf, entry_path: String) -> Option<u32> {
	let mut lock = PREVIEW_GMA.lock();
	if let Some(gma) = lock.as_mut() {
		let transaction = crate::transactions::new_extraction();
		let id = transaction.id;
		if *gma.path != gma_path {
			let loaded = GMAFile::open(gma_path).and_then(|mut gma| {
				gma.entries()?;
				Ok(gma)
			});
			match loaded {
				Ok(loaded) => *gma = Arc::new(loaded),
				Err(error) => {
					transaction.error(error.to_string(), turbonone!());
					return Some(id);
				}
			}
		}

		let gma_ref = gma.clone();
		rayon::spawn(move || {
			ignore! { gma_ref.extract_entry(entry_path, &transaction, true) };
		});

		Some(id)
	} else {
		None
	}
}

#[tauri::command]
pub fn extract_preview_gma(gma_path: PathBuf, dest: ExtractDestination) -> Option<u32> {
	let mut lock = PREVIEW_GMA.lock();
	if let Some(gma) = lock.as_mut() {
		let transaction = crate::transactions::new_extraction();
		let id = transaction.id;
		if *gma.path != gma_path {
			let loaded = GMAFile::open(gma_path).and_then(|mut gma| {
				gma.entries()?;
				Ok(gma)
			});
			match loaded {
				Ok(loaded) => *gma = Arc::new(loaded),
				Err(error) => {
					transaction.error(error.to_string(), turbonone!());
					return Some(id);
				}
			}
		}

		let gma_ref = gma.clone();
		rayon::spawn(move || {
			ignore! { gma_ref.extract(dest, &transaction, true, true) };
		});

		Some(id)
	} else {
		None
	}
}

const TEXT_PREVIEW_LIMIT: u64 = 1024 * 1024;
const MEDIA_PREVIEW_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct EntryPreview {
	kind: &'static str,
	mime: &'static str,
	data: String,
}

fn preview_format(path: &str) -> Option<(&'static str, &'static str)> {
	let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
	Some(match extension.as_str() {
		"lua" => ("lua", "text/plain"),
		"txt" | "json" | "vmt" | "cfg" | "properties" | "csv" | "md" => ("text", "text/plain"),
		"png" => ("image", "image/png"),
		"jpg" | "jpeg" => ("image", "image/jpeg"),
		"gif" => ("image", "image/gif"),
		"webp" => ("image", "image/webp"),
		"bmp" => ("image", "image/bmp"),
		"ico" => ("image", "image/x-icon"),
		"wav" => ("audio", "audio/wav"),
		"mp3" => ("audio", "audio/mpeg"),
		"ogg" => ("audio", "audio/ogg"),
		"flac" => ("audio", "audio/flac"),
		"m4a" => ("audio", "audio/mp4"),
		"aac" => ("audio", "audio/aac"),
		"mp4" => ("video", "video/mp4"),
		"webm" => ("video", "video/webm"),
		_ => return None,
	})
}

fn entry_preview(gma: &GMAFile, entry_path: &str, mut reader: GMAReader) -> Result<EntryPreview, String> {
	let entry = gma.entries.as_ref().and_then(|entries| entries.get(entry_path)).ok_or_else(|| GMAError::EntryNotFound.to_string())?;
	let (kind, mime) = preview_format(entry_path).ok_or("ERR_FILE_PREVIEW_UNSUPPORTED")?;
	check_preview_size(kind, entry.size)?;
	let offset = gma.pointers.entries.checked_add(entry.index).ok_or_else(|| GMAError::FormatError.to_string())?;
	reader.seek(SeekFrom::Start(offset)).map_err(|error| GMAError::io("seek preview entry", &gma.path, error).to_string())?;
	let mut bytes = vec![0; entry.size as usize];
	reader.read_exact(&mut bytes).map_err(|error| GMAError::io("read preview entry", &gma.path, error).to_string())?;
	if entry.crc != 0 && crc32fast::hash(&bytes) != entry.crc {
		return Err(GMAError::Checksum(entry.path.clone()).to_string());
	}
	decode_preview(kind, mime, bytes)
}

fn check_preview_size(kind: &str, size: u64) -> Result<u64, String> {
	let text = matches!(kind, "text" | "lua");
	let limit = if text { TEXT_PREVIEW_LIMIT } else { MEDIA_PREVIEW_LIMIT };
	if size > limit {
		return Err(if text { "ERR_FILE_PREVIEW_TEXT_LIMIT" } else { "ERR_FILE_PREVIEW_MEDIA_LIMIT" }.into());
	}
	Ok(limit)
}

fn decode_preview(kind: &'static str, mime: &'static str, bytes: Vec<u8>) -> Result<EntryPreview, String> {
	let data = if matches!(kind, "text" | "lua") {
		let source = String::from_utf8(bytes).map_err(|_| "ERR_FILE_PREVIEW_ENCODING")?;
		if source.contains('\0') { return Err("ERR_FILE_PREVIEW_ENCODING".into()); }
		source.strip_prefix('\u{feff}').unwrap_or(&source).to_owned()
	} else {
		base64::encode(bytes)
	};
	Ok(EntryPreview { kind, mime, data })
}

#[tauri::command]
pub fn preview_gma_entry(gma_path: PathBuf, entry_path: String) -> Result<EntryPreview, String> {
	let mut gma = GMAFile::open(gma_path).map_err(|error| error.to_string())?;
	let reader = gma.entries().map_err(|error| error.to_string())?.ok_or_else(|| GMAError::FormatError.to_string())?;
	entry_preview(&gma, &entry_path, reader)
}

fn folder_entry_preview(content_path: &Path, entry_path: &str, ignore: &[String]) -> Result<EntryPreview, String> {
	let (kind, mime) = preview_format(entry_path).ok_or("ERR_FILE_PREVIEW_UNSUPPORTED")?;
	let manifest = ContentManifest::build(content_path, ignore, || false).map_err(|error| error.to_string())?;
	let entry = manifest.into_entries().into_iter().find(|entry| entry.archive_path == entry_path).ok_or_else(|| GMAError::EntryNotFound.to_string())?;
	check_preview_size(kind, entry.size)?;
	let file_error = |error| GMAError::io("read preview file", &entry.source_path, error).to_string();
	let relative = entry.source_path.strip_prefix(content_path).map_err(|_| GMAError::InvalidContentPath.to_string())?;
	let mut parts = relative.components().peekable();
	let mut directory = Directory::open(content_path, false).map_err(file_error)?;
	while let Some(part) = parts.next() {
		if parts.peek().is_none() {
			let file = directory.read_file(Path::new(part.as_os_str())).map_err(file_error)?;
			let limit = check_preview_size(kind, file.metadata().map_err(file_error)?.len())?;
			let mut bytes = Vec::new();
			file.take(limit + 1).read_to_end(&mut bytes).map_err(file_error)?;
			check_preview_size(kind, bytes.len() as u64)?;
			return decode_preview(kind, mime, bytes);
		}
		directory = directory.child(Path::new(part.as_os_str()), false).map_err(file_error)?;
	}
	Err(GMAError::EntryNotFound.to_string())
}

#[tauri::command]
pub fn preview_folder_entry(content_path: PathBuf, entry_path: String) -> Result<EntryPreview, String> {
	let ignore = app_data!().settings.read().ignore_globs.clone();
	folder_entry_preview(&content_path, &entry_path, &ignore)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::NTStringWriter;
	use byteorder::{LittleEndian, WriteBytesExt};
	use std::io::Cursor;

	fn archive(path: &str, payload: &[u8], crc: u32) -> GMAFile {
		let mut bytes = b"GMAD\x03".to_vec();
		bytes.extend_from_slice(&[0; 17]);
		for text in ["Preview", "{}", "Author"] { bytes.write_nt_string(text).unwrap(); }
		bytes.write_i32::<LittleEndian>(1).unwrap();
		bytes.write_u32::<LittleEndian>(1).unwrap();
		bytes.write_nt_string(path).unwrap();
		bytes.write_i64::<LittleEndian>(payload.len() as i64).unwrap();
		bytes.write_u32::<LittleEndian>(crc).unwrap();
		bytes.write_u32::<LittleEndian>(0).unwrap();
		bytes.extend_from_slice(payload);
		let mut gma = GMAFile::read_header(GMAReader::MemBuffer(Cursor::new(bytes.into())), "preview.gma").unwrap();
		gma.entries().unwrap();
		gma
	}

	fn read_preview(gma: &GMAFile, path: &str) -> Result<EntryPreview, String> {
		entry_preview(gma, path, gma.read().unwrap())
	}

	#[test]
	fn previews_folder_entries_using_the_manifest_source_path() {
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path();
		std::fs::create_dir_all(root.join("LUA/Nested")).unwrap();
		std::fs::create_dir(root.join("sound")).unwrap();
		let source = "-- café\nprint('<script>')\n";
		std::fs::write(root.join("LUA/Nested/Test.LUA"), source).unwrap();
		std::fs::write(root.join("sound/test.wav"), b"\x00\xff\x10").unwrap();
		let preview = folder_entry_preview(root, "lua/nested/test.lua", &[]).unwrap();
		assert_eq!((preview.kind, preview.data.as_str()), ("lua", source));
		let preview = folder_entry_preview(root, "sound/test.wav", &[]).unwrap();
		assert_eq!((preview.kind, preview.mime), ("audio", "audio/wav"));
		assert_eq!(base64::decode(preview.data).unwrap(), b"\x00\xff\x10");
		assert_eq!(folder_entry_preview(root, "lua/nested/test.lua", &["lua/*".into()]).unwrap_err(), "ERR_GMA_ENTRY_NOT_FOUND");
		for path in ["../outside.lua", "lua/../../outside.lua", "/outside.lua", "C:/outside.lua", "lua/missing.lua"] {
			assert_eq!(folder_entry_preview(root, path, &[]).unwrap_err(), "ERR_GMA_ENTRY_NOT_FOUND");
		}
		assert_eq!(folder_entry_preview(root, "model.mdl", &[]).unwrap_err(), "ERR_FILE_PREVIEW_UNSUPPORTED");
	}

	#[test]
	fn rejects_oversized_and_binary_folder_previews() {
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path();
		std::fs::create_dir(root.join("lua")).unwrap();
		std::fs::create_dir(root.join("sound")).unwrap();
		for (path, size, error) in [("lua/test.lua", TEXT_PREVIEW_LIMIT, "ERR_FILE_PREVIEW_TEXT_LIMIT"), ("sound/test.wav", MEDIA_PREVIEW_LIMIT, "ERR_FILE_PREVIEW_MEDIA_LIMIT")] {
			std::fs::File::create(root.join(path)).unwrap().set_len(size + 1).unwrap();
			assert_eq!(folder_entry_preview(root, path, &[]).unwrap_err(), error);
		}
		for bytes in [b"\xff".as_slice(), b"\0binary"] {
			std::fs::write(root.join("lua/test.lua"), bytes).unwrap();
			assert_eq!(folder_entry_preview(root, "lua/test.lua", &[]).unwrap_err(), "ERR_FILE_PREVIEW_ENCODING");
		}
		assert!(folder_entry_preview(Path::new("relative"), "lua/test.lua", &[]).is_err());
	}

	#[test]
	fn previews_lua_as_text_and_preserves_media_bytes() {
		let source = "-- café\nprint('<script>')\n";
		let gma = archive("lua/test.lua", source.as_bytes(), crc32fast::hash(source.as_bytes()));
		let preview = read_preview(&gma, "lua/test.lua").unwrap();
		assert_eq!((preview.kind, preview.data.as_str()), ("lua", source));
		for (path, kind, mime) in [("img.GIF", "image", "image/gif"), ("sound/a.wav", "audio", "audio/wav"), ("video.webm", "video", "video/webm")] {
			let bytes = b"\x00\xff\x10";
			let preview = read_preview(&archive(path, bytes, 0), path).unwrap();
			assert_eq!((preview.kind, preview.mime), (kind, mime));
			assert_eq!(base64::decode(preview.data).unwrap(), bytes);
		}
	}

	#[test]
	fn rejects_missing_unsupported_binary_and_corrupt_entries() {
		let gma = archive("lua/test.lua", b"test", 1);
		assert!(read_preview(&gma, "lua/test.lua").unwrap_err().starts_with("ERR_GMA_CHECKSUM:"));
		assert_eq!(read_preview(&gma, "../outside.lua").unwrap_err(), "ERR_GMA_ENTRY_NOT_FOUND");
		for path in ["file.html", "file.svg", "model.mdl"] {
			assert_eq!(read_preview(&archive(path, b"test", 0), path).unwrap_err(), "ERR_FILE_PREVIEW_UNSUPPORTED");
		}
		for bytes in [b"\xff".as_slice(), b"\0binary"] {
			assert_eq!(read_preview(&archive("lua/test.lua", bytes, 0), "lua/test.lua").unwrap_err(), "ERR_FILE_PREVIEW_ENCODING");
		}
	}

	#[test]
	fn bounds_preview_sizes_before_allocating_or_reading() {
		for (path, limit, error) in [("test.lua", TEXT_PREVIEW_LIMIT, "ERR_FILE_PREVIEW_TEXT_LIMIT"), ("sound.wav", MEDIA_PREVIEW_LIMIT, "ERR_FILE_PREVIEW_MEDIA_LIMIT")] {
			let mut gma = archive(path, b"", 0);
			gma.entries.as_mut().unwrap().get_mut(path).unwrap().size = limit + 1;
			assert_eq!(read_preview(&gma, path).unwrap_err(), error);
		}
	}

	#[test]
	fn reads_exact_entry_range_and_reports_truncation() {
		let mut gma = archive("test.lua", b"text", 0);
		gma.entries.as_mut().unwrap().get_mut("test.lua").unwrap().size += 1;
		assert!(read_preview(&gma, "test.lua").unwrap_err().starts_with("ERR_IO_ERROR:read preview entry"));
		let mut gma = archive("test.lua", b"text", 0);
		gma.entries.as_mut().unwrap().get_mut("test.lua").unwrap().index = u64::MAX;
		assert_eq!(read_preview(&gma, "test.lua").unwrap_err(), "ERR_GMA_FORMAT_ERROR");
	}
}
