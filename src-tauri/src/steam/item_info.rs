use std::{
	io::Read,
	sync::Arc,
	time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use steamworks::{Client, PersonaChange, PersonaStateChange, PublishedFileId, QueryResult, QueryResults, SteamError, SteamId};

use crate::{
	gma::{workshop::{WorkshopIcon, WorkshopInfo, WorkshopItemDetails}, ExtractOptions},
	Transaction, GMOD_APP_ID,
};

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(30);
const ICON_MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, PartialEq)]
enum Wait<T> {
	Ready(T),
	TimedOut,
	Cancelled,
}

/// A callback's reply slot. Once the waiter gives up, late replies are discarded.
struct Reply<T>(Mutex<(bool, Option<T>)>);
impl<T> Reply<T> {
	fn new() -> Arc<Self> {
		Arc::new(Self(Mutex::new((false, None))))
	}

	fn send(&self, value: T) {
		let mut slot = self.0.lock();
		if !slot.0 {
			slot.1 = Some(value);
		}
	}

	fn wait(&self, deadline: Instant, cancelled: &dyn Fn() -> bool, pump: &mut dyn FnMut()) -> Wait<T> {
		loop {
			if let Some(value) = self.0.lock().1.take() {
				return Wait::Ready(value);
			}
			let outcome = if cancelled() {
				Wait::Cancelled
			} else if Instant::now() >= deadline {
				Wait::TimedOut
			} else {
				pump();
				continue;
			};
			let mut slot = self.0.lock();
			slot.0 = true;
			return match slot.1.take() {
				Some(value) if !matches!(outcome, Wait::Cancelled) => Wait::Ready(value),
				_ => outcome,
			};
		}
	}
}

fn item_details(requested: PublishedFileId, result: Option<QueryResult>, preview_url: Option<String>, retrieved: DateTime<Utc>) -> Result<WorkshopItemDetails, String> {
	let item = result.ok_or("ERR_WORKSHOP_ITEM_MISSING")?;
	if item.published_file_id != requested || item.consumer_app_id != Some(GMOD_APP_ID) {
		return Err("ERR_WORKSHOP_ITEM_MISMATCH".into());
	}
	Ok(WorkshopItemDetails {
		title: item.title,
		description: item.description,
		owner: item.owner,
		preview_url: preview_url.filter(|url| !url.is_empty()),
		retrieved,
	})
}

/// Validates a result from a download batch query.
pub fn query_details(requested: PublishedFileId, results: &QueryResults<'_>, index: u32) -> Result<WorkshopItemDetails, String> {
	item_details(requested, results.get(index), results.preview_url(index), Utc::now())
}

fn query(client: &Client, id: PublishedFileId, deadline: Instant, cancelled: &dyn Fn() -> bool, pump: &mut dyn FnMut()) -> Wait<Result<WorkshopItemDetails, String>> {
	let query = match client.ugc().query_item(id) {
		Ok(query) => query,
		Err(error) => return Wait::Ready(Err(format!("ERR_WORKSHOP_LOOKUP:{error}"))),
	};
	let reply = Reply::new();
	let sender = reply.clone();
	query
		.include_long_desc(true)
		.allow_cached_response(0)
		.fetch(move |result: Result<QueryResults<'_>, SteamError>| {
			sender.send(result.map(|results| item_details(id, results.get(0), results.preview_url(0), Utc::now())).unwrap_or_else(|error| Err(format!("ERR_WORKSHOP_LOOKUP:{error}"))));
		});
	reply.wait(deadline, cancelled, pump)
}

fn owner_name(client: &Client, owner: SteamId, deadline: Instant, cancelled: &dyn Fn() -> bool, pump: &mut dyn FnMut()) -> Wait<Option<String>> {
	let reply = Reply::new();
	let sender = reply.clone();
	let _callback = client.register_callback(move |change: PersonaStateChange| {
		if change.steam_id == owner && change.flags & PersonaChange::NAME == PersonaChange::NAME {
			sender.send(());
		}
	});
	let friends = client.friends();
	if friends.request_user_information(owner, true) {
		match reply.wait(deadline, cancelled, pump) {
			Wait::Ready(()) => {}
			Wait::TimedOut => return Wait::TimedOut,
			Wait::Cancelled => return Wait::Cancelled,
		}
	}
	let name = friends.get_friend(owner).name();
	Wait::Ready((!name.is_empty() && name != "[unknown]").then_some(name))
}

/// Gathers Workshop details for an extraction. Returns None when cancelled.
fn collect(
	client: Option<&Client>,
	id: Option<PublishedFileId>,
	id_inferred: bool,
	prefetched: Option<Result<WorkshopItemDetails, String>>,
	cancelled: &dyn Fn() -> bool,
	pump: &mut dyn FnMut(),
) -> Option<(WorkshopInfo, Vec<String>)> {
	let mut info = WorkshopInfo { id, id_inferred, ..Default::default() };
	let Some(id) = id else { return Some((info, vec!["ERR_WORKSHOP_ID_UNKNOWN".into()])) };
	let deadline = Instant::now() + LOOKUP_TIMEOUT;
	let item = match (prefetched, client) {
		(Some(item), _) => item,
		(None, None) => Err("ERR_WORKSHOP_STEAM_UNAVAILABLE".into()),
		(None, Some(client)) => match query(client, id, deadline, cancelled, pump) {
			Wait::Ready(item) => item,
			Wait::TimedOut => Err("ERR_WORKSHOP_LOOKUP_TIMEOUT".into()),
			Wait::Cancelled => return None,
		},
	};
	let mut warnings = Vec::new();
	match item {
		Err(warning) => warnings.push(warning),
		Ok(item) => {
			let name = match client {
				Some(client) => owner_name(client, item.owner, deadline, cancelled, pump),
				None => Wait::Ready(None),
			};
			match name {
				Wait::Ready(Some(name)) => info.owner_name = Some(name),
				Wait::Ready(None) | Wait::TimedOut => warnings.push("ERR_WORKSHOP_OWNER_UNAVAILABLE".into()),
				Wait::Cancelled => return None,
			}
			info.item = Some(item);
		}
	}
	Some((info, warnings))
}

fn icon_from_bytes(bytes: Vec<u8>) -> Result<WorkshopIcon, String> {
	let extension = match image::guess_format(&bytes) {
		Ok(image::ImageFormat::Jpeg) => "jpg",
		Ok(image::ImageFormat::Png) => "png",
		Ok(image::ImageFormat::Gif) => "gif",
		_ => return Err("ERR_WORKSHOP_ICON_FORMAT".into()),
	};
	Ok(WorkshopIcon { bytes, extension })
}

fn download_icon(url: &str) -> Result<WorkshopIcon, String> {
	let response = ureq::get(url).timeout(LOOKUP_TIMEOUT).call().map_err(|error| format!("ERR_WORKSHOP_ICON_DOWNLOAD:{error}"))?;
	let mut bytes = Vec::new();
	response.into_reader().take(ICON_MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|error| format!("ERR_WORKSHOP_ICON_DOWNLOAD:{error}"))?;
	if bytes.len() as u64 > ICON_MAX_BYTES {
		return Err("ERR_WORKSHOP_ICON_TOO_LARGE".into());
	}
	icon_from_bytes(bytes)
}

/// Attaches Workshop details for the desktop's extraction jobs. Returns false when cancelled.
pub fn attach(options: &mut ExtractOptions, id: Option<PublishedFileId>, id_inferred: bool, prefetched: Option<Result<WorkshopItemDetails, String>>, transaction: &Transaction) -> bool {
	if !options.enabled() {
		return true;
	}
	transaction.status("fetching_workshop_info");
	let client = steam!().connected().then(|| steam!().client());
	let Some((mut info, warnings)) = collect(client.as_deref().map(|interface| &**interface), id, id_inferred, prefetched, &|| transaction.aborted(), &mut || steam!().run_callbacks()) else {
		return false;
	};
	for warning in warnings {
		transaction.warning(warning);
	}
	if let Some(url) = info.item.as_ref().filter(|_| options.icon).map(|item| item.preview_url.clone()) {
		match url.ok_or_else(|| "ERR_WORKSHOP_ICON_MISSING".to_owned()).and_then(|url| download_icon(&url)) {
			Ok(icon) => info.icon = Some(icon),
			Err(warning) => transaction.warning(warning),
		}
		if transaction.aborted() {
			return false;
		}
	}
	options.workshop = Some(info);
	true
}

/// The CLI has no desktop Steam connection, so it uses one scoped client for the lookup.
pub fn attach_cli(options: &mut ExtractOptions, id: Option<PublishedFileId>, id_inferred: bool, transaction: &Transaction) {
	if !options.enabled() {
		return;
	}
	let connection = if id.is_some() { Client::init_app(GMOD_APP_ID).ok() } else { None };
	#[cfg(target_os = "windows")]
	{
		// See Steam::connect: do not let opened folders inherit Garry's Mod's Steam identity.
		std::env::remove_var("SteamAppId");
		std::env::remove_var("SteamGameId");
	}
	let (info, warnings) = match &connection {
		Some((client, single)) => collect(Some(client), id, id_inferred, None, &|| false, &mut || {
			single.run_callbacks();
			sleep_ms!(50);
		}),
		None => collect(None, id, id_inferred, None, &|| false, &mut || {}),
	}
	.expect("CLI lookups are not cancellable");
	for warning in warnings {
		transaction.warning(warning);
	}
	options.workshop = Some(info);
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::Cell;

	fn result(id: u64, app: Option<u32>) -> QueryResult {
		QueryResult {
			published_file_id: PublishedFileId(id),
			creator_app_id: app.map(steamworks::AppId),
			consumer_app_id: app.map(steamworks::AppId),
			title: "Addon".into(),
			description: "[b]Description[/b]".into(),
			owner: SteamId::from_raw(76561197960287930),
			time_created: 0,
			time_updated: 0,
			time_added_to_user_list: 0,
			visibility: steamworks::PublishedFileVisibility::Public,
			banned: false,
			accepted_for_use: true,
			tags: Vec::new(),
			tags_truncated: false,
			file_type: steamworks::FileType::Community,
			file_size: 0,
			url: String::new(),
			num_upvotes: 0,
			num_downvotes: 0,
			score: 0.,
			num_children: 0,
		}
	}

	#[test]
	fn details_reject_missing_wrong_and_non_gmod_items() {
		let now = Utc::now();
		let item = item_details(PublishedFileId(7), Some(result(7, Some(4000))), Some("https://example.com/icon.jpg".into()), now).unwrap();
		assert_eq!((item.title.as_str(), item.description.as_str(), item.retrieved), ("Addon", "[b]Description[/b]", now));
		assert_eq!(item.preview_url.as_deref(), Some("https://example.com/icon.jpg"));
		assert_eq!(item_details(PublishedFileId(7), Some(result(7, Some(4000))), Some(String::new()), now).unwrap().preview_url, None);
		assert_eq!(item_details(PublishedFileId(7), None, None, now).unwrap_err(), "ERR_WORKSHOP_ITEM_MISSING");
		for wrong in [result(8, Some(4000)), result(7, Some(4001)), result(7, None)] {
			assert_eq!(item_details(PublishedFileId(7), Some(wrong), None, now).unwrap_err(), "ERR_WORKSHOP_ITEM_MISMATCH");
		}
	}

	#[test]
	fn icons_keep_their_image_format_and_reject_other_data() {
		for (bytes, extension) in [(&b"\xFF\xD8\xFFdata"[..], "jpg"), (b"\x89PNG\r\n\x1a\ndata", "png"), (b"GIF89adata", "gif")] {
			let icon = icon_from_bytes(bytes.to_vec()).unwrap();
			assert_eq!((icon.bytes.as_slice(), icon.extension), (bytes, extension));
		}
		for bytes in [&b"<html>"[..], b"RIFF\0\0\0\0WEBPdata", b""] {
			assert_eq!(icon_from_bytes(bytes.to_vec()).unwrap_err(), "ERR_WORKSHOP_ICON_FORMAT");
		}
	}

	#[test]
	fn waits_stop_on_cancellation_and_timeout_and_discard_late_replies() {
		let reply = Reply::<u32>::new();
		let sender = reply.clone();
		let pumps = Cell::new(0);
		let ready = reply.wait(Instant::now() + Duration::from_secs(5), &|| false, &mut || {
			pumps.set(pumps.get() + 1);
			if pumps.get() == 3 { sender.send(1); }
		});
		assert_eq!(ready, Wait::Ready(1));

		let reply = Reply::<u32>::new();
		let cancelled = Cell::new(false);
		assert_eq!(reply.wait(Instant::now() + Duration::from_secs(5), &|| cancelled.get(), &mut || cancelled.set(true)), Wait::Cancelled);
		reply.send(2);
		assert_eq!(reply.0.lock().1, None);

		let reply = Reply::<u32>::new();
		let started = Instant::now();
		assert_eq!(reply.wait(started + Duration::from_millis(20), &|| false, &mut || std::thread::sleep(Duration::from_millis(5))), Wait::TimedOut);
		assert!(started.elapsed() >= Duration::from_millis(20));
		reply.send(3);
		assert_eq!(reply.0.lock().1, None);
	}

	fn unused_pump() {
		panic!("no Steam client to pump");
	}

	#[test]
	fn collection_reports_partial_information_without_steam() {
		let (info, warnings) = collect(None, None, true, None, &|| false, &mut unused_pump).unwrap();
		assert!(info.id.is_none() && info.item.is_none());
		assert_eq!(warnings, ["ERR_WORKSHOP_ID_UNKNOWN"]);

		let (info, warnings) = collect(None, Some(PublishedFileId(7)), true, None, &|| false, &mut unused_pump).unwrap();
		assert_eq!((info.id, info.id_inferred, info.item.is_none()), (Some(PublishedFileId(7)), true, true));
		assert_eq!(warnings, ["ERR_WORKSHOP_STEAM_UNAVAILABLE"]);

		let prefetched = item_details(PublishedFileId(7), Some(result(7, Some(4000))), None, Utc::now());
		let (info, warnings) = collect(None, Some(PublishedFileId(7)), false, Some(prefetched), &|| false, &mut unused_pump).unwrap();
		assert_eq!(info.item.unwrap().title, "Addon");
		assert_eq!(info.owner_name, None);
		assert_eq!(warnings, ["ERR_WORKSHOP_OWNER_UNAVAILABLE"]);

		let (info, warnings) = collect(None, Some(PublishedFileId(7)), false, Some(Err("ERR_WORKSHOP_ITEM_MISMATCH".into())), &|| false, &mut unused_pump).unwrap();
		assert!(info.item.is_none());
		assert_eq!(warnings, ["ERR_WORKSHOP_ITEM_MISMATCH"]);
	}
}
