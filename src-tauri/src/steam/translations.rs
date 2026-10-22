use std::{
	collections::{BTreeMap, HashMap},
	path::PathBuf,
	sync::{mpsc, Arc},
	time::Duration,
};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use steamworks::PublishedFileId;

use super::{
	publishing::{validate_description, PublishError},
	Steam,
};
use crate::{Transaction, GMOD_APP_ID};

// The steamworks crate asserts that Steam accepts a language, so only these API codes may reach it. English is the main text.
const LANGUAGES: [(&str, &str); 30] = [
	("arabic", "ar"),
	("bulgarian", "bg"),
	("schinese", "zh-CN"),
	("tchinese", "zh-TW"),
	("czech", "cs"),
	("danish", "da"),
	("dutch", "nl"),
	("finnish", "fi"),
	("french", "fr"),
	("german", "de"),
	("greek", "el"),
	("hungarian", "hu"),
	("indonesian", "id"),
	("italian", "it"),
	("japanese", "ja"),
	("koreana", "ko"),
	("malay", "ms"),
	("norwegian", "no"),
	("polish", "pl"),
	("portuguese", "pt"),
	("brazilian", "pt-BR"),
	("romanian", "ro"),
	("russian", "ru"),
	("spanish", "es"),
	("latam", "es-419"),
	("swedish", "sv"),
	("thai", "th"),
	("turkish", "tr"),
	("ukrainian", "uk"),
	("vietnamese", "vi"),
];

// k_cchPublishedDocumentTitleMax is 129, including the C string terminator.
pub(super) const TITLE_MAX_BYTES: usize = 128;
const STEAM_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Translation {
	pub title: String,
	pub description: String,
}

impl Translation {
	fn is_empty(&self) -> bool {
		self.title.is_empty() && self.description.is_empty()
	}
}

pub type Translations = BTreeMap<String, Translation>;

#[derive(Serialize)]
pub struct TranslationFailure {
	language: String,
	error: String,
}

fn validate_language(language: &str) -> Result<(), PublishError> {
	if LANGUAGES.iter().any(|(code, _)| *code == language) {
		Ok(())
	} else {
		Err(PublishError::TranslationLanguage)
	}
}

pub fn validate(translations: &Translations) -> Result<(), PublishError> {
	for (language, translation) in translations {
		validate_language(language)?;
		if translation.title.len() > TITLE_MAX_BYTES {
			return Err(PublishError::TitleTooLong);
		}
		if translation.title.contains('\0') {
			return Err(PublishError::TitleContainsNul);
		}
		validate_description(Some(&translation.description))?;
	}
	Ok(())
}

impl Steam {
	fn update_translation(&self, id: PublishedFileId, language: &str, translation: &Translation) -> Result<(), PublishError> {
		let result = Arc::new(Mutex::new(None));
		let result_ref = result.clone();
		let mut update = self.client().ugc().start_item_update(GMOD_APP_ID, id).language(language);
		if !translation.title.is_empty() {
			update = update.title(&translation.title);
		}
		if !translation.description.is_empty() {
			update = update.description(&translation.description);
		}
		update.submit(None, move |reply| {
			*result_ref.lock() = Some(reply);
		});
		loop {
			let reply = result.lock().take();
			if let Some(reply) = reply {
				return reply.map(|_| ()).map_err(PublishError::from);
			}
			self.run_callbacks();
		}
	}
}

// A failed language keeps its draft without failing the job, because sending it again only replaces that language's text.
pub fn publish(id: PublishedFileId, translations: &Translations, transaction: &Transaction) -> (Translations, Vec<TranslationFailure>) {
	let pending: Vec<_> = translations.iter().filter(|(_, translation)| !translation.is_empty()).collect();
	let mut sent = Translations::new();
	let mut failed = Vec::new();
	if pending.is_empty() {
		return (sent, failed);
	}
	transaction.status("PUBLISH_UPDATING_TRANSLATIONS");
	transaction.progress_reset();
	for (index, (language, translation)) in pending.iter().enumerate() {
		match steam!().update_translation(id, language, translation) {
			Ok(()) => {
				sent.insert((*language).clone(), (*translation).clone());
			}
			Err(error) => {
				eprintln!("Could not send the {language} translation of Workshop item {}: {error}", id.0);
				failed.push(TranslationFailure {
					language: (*language).clone(),
					error: error.to_string(),
				});
			}
		}
		transaction.progress((index + 1) as f64 / pending.len() as f64);
	}
	(sent, failed)
}

pub fn forget_published(drafts: &mut HashMap<String, Translations>, source: Option<&str>, id: PublishedFileId, sent: &Translations) {
	let target = format!("workshop:{}", id.0);
	let Some(mut remaining) = drafts.remove(source.unwrap_or(&target)) else {
		return;
	};
	remaining.retain(|language, draft| sent.get(language) != Some(draft));
	if !remaining.is_empty() {
		drafts.entry(target).or_default().extend(remaining);
	}
}

#[derive(Serialize)]
pub struct TranslationDrafts {
	key: String,
	translations: Translations,
}

#[tauri::command]
pub fn workshop_languages() -> Vec<(&'static str, &'static str)> {
	LANGUAGES.to_vec()
}

#[tauri::command]
pub fn get_translations(addon_id: Option<PublishedFileId>, content_path: Option<PathBuf>) -> Result<TranslationDrafts, String> {
	let key = crate::appdata::changelog_key(addon_id, content_path)?;
	let translations = app_data!().settings.read().translations.get(&key).cloned().unwrap_or_default();
	Ok(TranslationDrafts { key, translations })
}

#[tauri::command]
pub fn save_translations(key: String, translations: Translations) -> Result<(), String> {
	crate::appdata::validate_changelog_key(&key)?;
	for language in translations.keys() {
		validate_language(language).map_err(|error| error.to_string())?;
	}
	crate::appdata::change_settings(|settings| {
		if translations.is_empty() {
			settings.translations.remove(&key);
		} else {
			settings.translations.insert(key, translations);
		}
		Ok(())
	})
}

fn request_text(id: PublishedFileId, language: &str) -> Result<mpsc::Receiver<Result<Translation, String>>, String> {
	let (sender, receiver) = mpsc::sync_channel(1);
	steam!()
		.client()
		.ugc()
		.query_item(id)
		.map_err(|error| error.to_string())?
		.language(language)
		.include_long_desc(true)
		.allow_cached_response(0)
		.fetch(move |result| {
			let text = result.map_err(|error| PublishError::from(error).to_string()).and_then(|results| {
				let item = results.get(0).ok_or("ERR_ITEM_NOT_FOUND")?;
				Ok(Translation {
					title: item.title,
					description: item.description,
				})
			});
			let _ = sender.send(text);
		});
	Ok(receiver)
}

fn receive_text(receiver: mpsc::Receiver<Result<Translation, String>>) -> Result<Translation, String> {
	receiver
		.recv_timeout(STEAM_TIMEOUT)
		.map_err(|_| "ERR_TRANSLATION_LOAD_TIMEOUT".to_owned())?
}

// Valve does not document what a query returns for a language without a translation, so text that matches the
// English text is treated as untranslated.
fn translated_only(english: Translation, translated: Translation) -> Translation {
	Translation {
		title: if translated.title == english.title {
			String::new()
		} else {
			translated.title
		},
		description: if translated.description == english.description {
			String::new()
		} else {
			translated.description
		},
	}
}

#[tauri::command]
pub async fn workshop_translation(addon_id: PublishedFileId, language: String) -> Result<Translation, String> {
	validate_language(&language).map_err(|error| error.to_string())?;
	tauri::async_runtime::spawn_blocking(move || {
		let english = request_text(addon_id, "english")?;
		let translated = request_text(addon_id, &language)?;
		Ok(translated_only(receive_text(english)?, receive_text(translated)?))
	})
	.await
	.map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
	use super::*;

	fn text(title: &str, description: &str) -> Translation {
		Translation {
			title: title.into(),
			description: description.into(),
		}
	}

	#[test]
	fn only_steam_languages_and_limits_are_accepted() {
		assert!(validate(&Translations::from([("german".into(), text("Titel", "Beschreibung"))])).is_ok());
		assert!(validate(&Translations::from([("english".into(), text("Title", ""))])).is_err());
		assert!(validate(&Translations::from([("de".into(), text("Titel", ""))])).is_err());
		assert!(validate(&Translations::from([("german".into(), text(&"a".repeat(128), ""))])).is_ok());
		assert!(validate(&Translations::from([("german".into(), text(&"a".repeat(129), ""))])).is_err());
		assert!(validate(&Translations::from([("german".into(), text("a\0", ""))])).is_err());
		assert!(validate(&Translations::from([("german".into(), text("", &"a".repeat(8000)))])).is_err());
	}

	#[test]
	fn published_drafts_are_dropped_and_the_rest_move_to_the_item() {
		let mut drafts = HashMap::from([(
			"path:C:\\addon".to_owned(),
			Translations::from([("german".into(), text("Titel", "")), ("french".into(), text("Titre", ""))]),
		)]);
		let sent = Translations::from([("german".into(), text("Titel", "")), ("french".into(), text("Ancien", ""))]);
		forget_published(&mut drafts, Some("path:C:\\addon"), PublishedFileId(7), &sent);
		assert_eq!(
			drafts,
			HashMap::from([("workshop:7".to_owned(), Translations::from([("french".into(), text("Titre", ""))]))])
		);

		forget_published(
			&mut drafts,
			None,
			PublishedFileId(7),
			&Translations::from([("french".into(), text("Titre", ""))]),
		);
		assert!(drafts.is_empty());
	}

	#[test]
	fn text_matching_english_is_not_a_translation() {
		assert_eq!(translated_only(text("Addon", "About"), text("Addon", "Über")), text("", "Über"));
		assert_eq!(translated_only(text("Addon", "About"), text("Addon", "About")), Translation::default());
	}
}
