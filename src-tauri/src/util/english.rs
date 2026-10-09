use serde_json::Value;

lazy_static! {
	static ref MESSAGES: Value = serde_json::from_str(include_str!("../../../i18n/en.json")).expect("i18n/en.json is valid JSON");
}

// Like svelte-i18n, a missing key falls back to the key itself.
pub fn text(key: &str) -> &str {
	key.split('.').try_fold(&*MESSAGES, |value, part| value.get(part)).and_then(Value::as_str).unwrap_or(key)
}

pub fn format(key: &str, values: &[(&str, &str)]) -> String {
	values.iter().fold(text(key).to_owned(), |text, (name, value)| text.replace(&format!("{{{name}}}"), value))
}

/// Mirrors `translateError` in app/i18n.js for `CODE:details` messages.
pub fn message(message: &str) -> String {
	let (key, details) = message.split_once(':').map_or((message, None), |(key, details)| (key, Some(details)));
	let text = format(key, &[("data", details.unwrap_or_default())]);
	match details {
		Some(details) if !details.is_empty() && !text.contains(details) => format!("{text}\n{details}"),
		_ => text,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn messages_use_english_resources_and_keep_details() {
		assert_eq!(text("settings.extraction.title"), "Extraction");
		assert_eq!(text("missing.key"), "missing.key");
		assert_eq!(format("workshop_export.title", &[("value", "Addon {value}")]), "Title: Addon {value}");
		assert_eq!(message("ERR_WORKSHOP_METADATA_RENAMED:workshop (2).txt"), "Workshop details were saved as workshop (2).txt because the usual name was taken.");
		assert_eq!(message("ERR_WORKSHOP_LOOKUP:Timeout"), format!("{}\nTimeout", text("ERR_WORKSHOP_LOOKUP")));
		assert_eq!(message("Unknown failure"), "Unknown failure");
	}
}
