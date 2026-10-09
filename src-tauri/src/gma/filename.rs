use steamworks::PublishedFileId;

const DEFAULT_GMA_FILE_NAME: &str = "publishedaddon";
pub const DEFAULT_METADATA_FILE_NAME: &str = "workshop.txt";
pub const GMA_FILE_NAME_MAX_CHARS: usize = 120;
const GMA_FILE_NAME_MAX_BYTES: usize = 251;
// Leaves room for a " (255)" collision suffix within the limits of a portable name.
const METADATA_STEM_MAX_BYTES: usize = 230;
const WORKSHOP_FOLDER_MAX_CHARS: usize = 120;
const WORKSHOP_FOLDER_MAX_BYTES: usize = 240;

fn portable_char(c: &char) -> bool {
	!c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
}

fn trim_end(name: &str) -> &str {
	name.trim_end_matches(|c: char| c == '.' || c.is_whitespace())
}

fn reserved(name: &str) -> bool {
	let base = name.split('.').next().unwrap().trim_end().to_ascii_uppercase();
	matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
		|| base
			.strip_prefix("COM")
			.or_else(|| base.strip_prefix("LPT"))
			.is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"))
}

fn truncate(name: &str, max_chars: usize, max_bytes: usize) -> String {
	let mut bytes = 0;
	name.chars()
		.take(max_chars)
		.take_while(|c| {
			bytes += c.len_utf8();
			bytes <= max_bytes
		})
		.collect()
}

fn sanitize_file_stem(name: &str, extension: &str, max_bytes: usize) -> Option<String> {
	let filtered: String = name.trim().chars().filter(portable_char).collect();
	let filtered = trim_end(filtered.trim());
	let stem = if filtered.to_ascii_lowercase().ends_with(extension) {
		&filtered[..filtered.len() - extension.len()]
	} else {
		filtered
	};
	let sanitized = truncate(stem, GMA_FILE_NAME_MAX_CHARS, max_bytes);
	let sanitized = trim_end(sanitized.trim());

	if sanitized.is_empty() || reserved(sanitized) {
		None
	} else {
		Some(sanitized.to_owned())
	}
}

#[tauri::command]
pub fn resolve_gma_file_name(gma_name: Option<&str>) -> String {
	let mut file_name = gma_name
		.and_then(|name| sanitize_file_stem(name, ".gma", GMA_FILE_NAME_MAX_BYTES))
		.unwrap_or_else(|| DEFAULT_GMA_FILE_NAME.to_owned());

	file_name.push_str(".gma");

	file_name
}

pub fn valid_gma_file_name(name: &str) -> bool {
	name.to_ascii_lowercase().ends_with(".gma") && sanitize_file_stem(name, ".gma", GMA_FILE_NAME_MAX_BYTES).as_deref() == Some(&name[..name.len() - 4])
}

pub fn valid_metadata_file_name(name: &str) -> bool {
	name.to_ascii_lowercase().ends_with(".txt") && sanitize_file_stem(name, ".txt", METADATA_STEM_MAX_BYTES).as_deref() == Some(&name[..name.len() - 4])
}

/// `workshop.txt` for 1, then `workshop (2).txt`, `workshop (3).txt`, ...
pub fn numbered_metadata_file_name(name: &str, number: u32) -> String {
	if number <= 1 {
		return name.to_owned();
	}
	let (stem, extension) = name.split_at(name.len() - 4);
	format!("{stem} ({number}){extension}")
}

/// `Workshop Title [123456789]`, truncating the title so the ID suffix always survives.
pub fn workshop_folder_name(title: &str, id: PublishedFileId) -> Option<String> {
	let suffix = format!(" [{}]", id.0);
	let filtered: String = title.chars().filter(portable_char).collect();
	let filtered = filtered.trim();
	let build = |prefix: &str| -> Option<String> {
		let title = truncate(
			&format!("{prefix}{filtered}"),
			WORKSHOP_FOLDER_MAX_CHARS - suffix.chars().count(),
			WORKSHOP_FOLDER_MAX_BYTES - suffix.len(),
		);
		let title = trim_end(title.trim_end());
		(title.len() > prefix.len()).then(|| format!("{title}{suffix}"))
	};
	let name = build("")?;
	if reserved(&name) { build("_") } else { Some(name) }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn portable_names_preserve_unicode_and_extensions() {
		for name in ["My addon.gma", "模型 🦀.gma", "COM10.GMA", "addon.tar.GmA", "addon.gma.gma"] {
			assert!(valid_gma_file_name(name), "{name}");
			assert_eq!(resolve_gma_file_name(Some(name)), format!("{}.gma", &name[..name.len() - 4]));
		}
		for name in [
			"CON.gma",
			"nul.tar.gma",
			"LPT².gma",
			"a:b.gma",
			"a?b.gma",
			"a\\b.gma",
			"a/b.gma",
			"a\0b.gma",
			"a\nb.gma",
			" .gma",
			"...gma",
			".gma",
			"a. .gma",
			" a.gma",
			"🦀",
		] {
			assert!(!valid_gma_file_name(name), "{name:?}");
		}
	}

	#[test]
	fn generated_names_are_valid_and_stable_at_length_limits() {
		for input in ["addon.gma?", "addon.gma. ", "addon.GM:A", "addon.gma\0"] {
			assert_eq!(resolve_gma_file_name(Some(input)), "addon.gma");
		}
		for input in ["a".repeat(130), "🦀".repeat(120), "界".repeat(120), format!("{}.GMA", "a".repeat(120))] {
			let name = resolve_gma_file_name(Some(&input));
			assert!(name.len() <= 255);
			assert!(valid_gma_file_name(&name));
			assert_eq!(resolve_gma_file_name(Some(&name)), name);
			assert!(!valid_gma_file_name(&format!("{input}.gma")));
		}
	}

	#[test]
	fn metadata_names_are_single_portable_text_files() {
		for name in ["workshop.txt", "Über 模型.TXT", "info.tar.txt", "COM10.txt"] {
			assert!(valid_metadata_file_name(name), "{name}");
		}
		for name in ["workshop", "workshop.md", ".txt", " .txt", "a. .txt", "CON.txt", "nul.tar.txt", "a/b.txt", "..\\b.txt", "a:b.txt", " a.txt", "a.txt ", &format!("{}.txt", "a".repeat(231))] {
			assert!(!valid_metadata_file_name(name), "{name:?}");
		}
		let longest = format!("{}.txt", "界".repeat(76));
		assert!(valid_metadata_file_name(&longest));
		assert!(numbered_metadata_file_name(&longest, 255).len() <= 240);
		assert_eq!(numbered_metadata_file_name("workshop.txt", 1), "workshop.txt");
		assert_eq!(numbered_metadata_file_name("Info.TXT", 2), "Info (2).TXT");
	}

	#[test]
	fn workshop_folder_names_keep_titles_and_complete_ids() {
		let id = PublishedFileId(123456789);
		assert_eq!(workshop_folder_name("  My Addon: Ünïcode 模型 🦀 ", id).unwrap(), "My Addon Ünïcode 模型 🦀 [123456789]");
		assert_eq!(workshop_folder_name("Trailing dots...", id).unwrap(), "Trailing dots [123456789]");
		assert_eq!(workshop_folder_name("a/b\\c<d>e|f?g*h\"i\n", id).unwrap(), "abcdefghi [123456789]");
		assert_eq!(workshop_folder_name("CON", id).unwrap(), "CON [123456789]");
		assert_eq!(workshop_folder_name("con.lua", id).unwrap(), "_con.lua [123456789]");
		assert_eq!(workshop_folder_name("LPT1 .x", id).unwrap(), "_LPT1 .x [123456789]");
		for title in ["", "   ", "...", "<>|"] {
			assert_eq!(workshop_folder_name(title, id), None, "{title:?}");
		}
		let id = PublishedFileId(u64::MAX);
		for title in ["a".repeat(300), "界".repeat(300), "🦀".repeat(300), format!("con.{}", "x".repeat(300)), format!("{} .", "a".repeat(95))] {
			let name = workshop_folder_name(&title, id).unwrap();
			assert!(name.chars().count() <= 120 && name.len() <= 240, "{name}");
			assert!(name.ends_with(&format!(" [{}]", u64::MAX)));
			assert!(!reserved(&name));
			assert!(!name[..name.len() - 23].ends_with([' ', '.']));
		}
		assert_ne!(workshop_folder_name("Same", PublishedFileId(1)), workshop_folder_name("Same", PublishedFileId(2)));
	}
}
