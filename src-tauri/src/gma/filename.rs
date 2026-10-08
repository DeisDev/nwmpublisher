const DEFAULT_GMA_FILE_NAME: &str = "publishedaddon";
pub const GMA_FILE_NAME_MAX_CHARS: usize = 120;
const GMA_FILE_NAME_MAX_BYTES: usize = 251;

fn sanitize_gma_file_name(name: &str) -> Option<String> {
	let filtered: String = name
		.trim()
		.chars()
		.filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
		.collect();
	let filtered = filtered.trim().trim_end_matches(|c: char| c == '.' || c.is_whitespace());
	let stem = if filtered.to_ascii_lowercase().ends_with(".gma") {
		&filtered[..filtered.len() - 4]
	} else {
		filtered
	};
	let mut bytes = 0;
	let sanitized: String = stem
		.chars()
		.take(GMA_FILE_NAME_MAX_CHARS)
		.take_while(|c| {
			bytes += c.len_utf8();
			bytes <= GMA_FILE_NAME_MAX_BYTES
		})
		.collect();

	let sanitized = sanitized.trim().trim_end_matches(|c: char| c == '.' || c.is_whitespace());
	let base = sanitized.split('.').next().unwrap().trim_end().to_ascii_uppercase();
	let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
		|| base
			.strip_prefix("COM")
			.or_else(|| base.strip_prefix("LPT"))
			.is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"));

	if sanitized.is_empty() || reserved {
		None
	} else {
		Some(sanitized.to_owned())
	}
}

#[tauri::command]
pub fn resolve_gma_file_name(gma_name: Option<&str>) -> String {
	let mut file_name = gma_name
		.and_then(sanitize_gma_file_name)
		.unwrap_or_else(|| DEFAULT_GMA_FILE_NAME.to_owned());

	file_name.push_str(".gma");

	file_name
}

pub fn valid_gma_file_name(name: &str) -> bool {
	name.to_ascii_lowercase().ends_with(".gma") && sanitize_gma_file_name(name).as_deref() == Some(&name[..name.len() - 4])
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
}
