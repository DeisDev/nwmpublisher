use chrono::{DateTime, SecondsFormat, Utc};
use steamworks::{PublishedFileId, SteamId};

use crate::util::english;

#[derive(Debug, Clone)]
pub struct WorkshopItemDetails {
	pub title: String,
	pub description: String,
	pub owner: SteamId,
	pub retrieved: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct WorkshopInfo {
	pub id: Option<PublishedFileId>,
	pub id_inferred: bool,
	pub item: Option<WorkshopItemDetails>,
	pub owner_name: Option<String>,
}

impl WorkshopInfo {
	pub fn folder_name(&self) -> Option<String> {
		super::filename::workshop_folder_name(&self.item.as_ref()?.title, self.id?)
	}

	pub fn render(&self, archive_title: Option<&str>) -> String {
		let unavailable = english::text("workshop_export.unavailable");
		let line = |key: &str, value: Option<&str>| english::format(key, &[("value", value.unwrap_or(unavailable))]);
		let item = self.item.as_ref();
		let id = self.id.map(|id| id.0.to_string());
		let owner = item.map(|item| item.owner.raw().to_string());
		let mut lines = vec![line("workshop_export.title", item.map(|item| item.title.as_str()))];
		if let Some(title) = archive_title.filter(|title| item.is_none() && !title.trim().is_empty()) {
			lines.push(line("workshop_export.archive_title", Some(title)));
		}
		lines.push(line(if self.id_inferred && id.is_some() { "workshop_export.id_inferred" } else { "workshop_export.id" }, id.as_deref()));
		lines.push(line("workshop_export.url", id.map(|id| format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}")).as_deref()));
		lines.push(line("workshop_export.owner", self.owner_name.as_deref()));
		lines.push(line("workshop_export.owner_id", owner.as_deref()));
		lines.push(line("workshop_export.owner_profile", owner.map(|owner| format!("https://steamcommunity.com/profiles/{owner}")).as_deref()));
		lines.push(line("workshop_export.retrieved", item.map(|item| item.retrieved.to_rfc3339_opts(SecondsFormat::Secs, true)).as_deref()));
		lines.push(String::new());
		match item {
			Some(item) => {
				lines.push(english::text("workshop_export.description").to_owned());
				lines.push(item.description.clone());
			}
			None => lines.push(english::text("workshop_export.description_unavailable").to_owned()),
		}
		let mut text = lines.join("\n");
		text.push('\n');
		text
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use chrono::TimeZone;

	fn details() -> WorkshopItemDetails {
		WorkshopItemDetails {
			title: "Café [b]Addon[/b]".into(),
			description: "[h1]Hello[/h1]\n[url=https://example.com/a?b=1]Link[/url]\n[img]https://example.com/i.png[/img]".into(),
			owner: SteamId::from_raw(76561197960287930),
			retrieved: Utc.with_ymd_and_hms(2026, 10, 9, 12, 34, 56).unwrap(),
		}
	}

	#[test]
	fn renders_complete_metadata_with_original_bbcode() {
		let info = WorkshopInfo { id: Some(PublishedFileId(123)), id_inferred: false, item: Some(details()), owner_name: Some("Owner Name".into()) };
		assert_eq!(
			info.render(Some("Archive")),
			"Title: Café [b]Addon[/b]\n\
			Workshop ID: 123\n\
			URL: https://steamcommunity.com/sharedfiles/filedetails/?id=123\n\
			Owner: Owner Name\n\
			Owner Steam ID: 76561197960287930\n\
			Owner profile: https://steamcommunity.com/profiles/76561197960287930\n\
			Retrieved (UTC): 2026-10-09T12:34:56Z\n\
			\n\
			Description:\n\
			[h1]Hello[/h1]\n[url=https://example.com/a?b=1]Link[/url]\n[img]https://example.com/i.png[/img]\n"
		);
		assert_eq!(info.folder_name().unwrap(), "Café [b]Addon[b] [123]");
	}

	#[test]
	fn renders_partial_metadata_and_marks_missing_fields() {
		let info = WorkshopInfo { id: Some(PublishedFileId(123)), id_inferred: true, item: Some(details()), owner_name: None };
		let text = info.render(None);
		assert!(text.contains("\nWorkshop ID: 123 (inferred from the file path)\n"));
		assert!(text.contains("\nOwner: Unavailable\nOwner Steam ID: 76561197960287930\nOwner profile: https://steamcommunity.com/profiles/76561197960287930\n"));

		let info = WorkshopInfo { id: Some(PublishedFileId(123)), id_inferred: true, item: None, owner_name: None };
		assert_eq!(
			info.render(Some("Embedded title")),
			"Title: Unavailable\n\
			Archive title: Embedded title\n\
			Workshop ID: 123 (inferred from the file path)\n\
			URL: https://steamcommunity.com/sharedfiles/filedetails/?id=123\n\
			Owner: Unavailable\n\
			Owner Steam ID: Unavailable\n\
			Owner profile: Unavailable\n\
			Retrieved (UTC): Unavailable\n\
			\n\
			Description: Unavailable\n"
		);
		assert_eq!(info.folder_name(), None);
		let text = WorkshopInfo::default().render(Some("  "));
		assert!(text.starts_with("Title: Unavailable\nWorkshop ID: Unavailable\nURL: Unavailable\n"));
	}
}
