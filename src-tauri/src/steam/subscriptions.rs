use std::{
	collections::{HashMap, HashSet},
	sync::{
		atomic::{AtomicU64, Ordering},
		mpsc, Arc,
	},
	time::{Duration, Instant, SystemTime},
};

use crate::{
	addon_cleaner::{missing_explained, SubscriptionPages},
	webview::Addon,
	WorkshopItem, GMOD_APP_ID,
};
use parking_lot::Mutex;
use serde::Serialize;
use steamworks::{AppIDs, PublishedFileId, QueryResults, SteamError, SteamId, UGCType, UserList, UserListOrder};
use tauri::ipc::Channel;

use super::{
	collections::{self, report, CollectionEntry, CollectionProgress, EntryStatus, ItemKind},
	workshop_management::{connected, query_item, query_items, read_item, STEAM_TIMEOUT},
	RESULTS_PER_PAGE,
};

const PAGES_IN_FLIGHT: usize = 4;
const INCOMPLETE: &str = "Steam returned an incomplete list of subscriptions. Try again.";
const SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(5 * 60);
static FETCH: Mutex<()> = Mutex::new(());
static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);

struct Cached {
	account: SteamId,
	fetched: Instant,
	snapshot: SubscriptionSnapshot,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionSnapshot {
	fetched_at: u64,
	items: Vec<CollectionEntry>,
}

/// Forgets the cached subscriptions after the app subscribes or unsubscribes.
pub(crate) fn invalidate() {
	GENERATION.fetch_add(1, Ordering::AcqRel);
	*CACHE.lock() = None;
}

type Page = Result<(u32, Vec<CollectionEntry>), String>;

fn request_page(account: SteamId, page: u32, sender: mpsc::Sender<(u32, Page)>) -> Result<(), String> {
	let failed = |error: SteamError| format!("Could not load your subscriptions: {error}");
	steam!()
		.client()
		.ugc()
		.query_user(
			account.account_id(),
			UserList::Subscribed,
			UGCType::All,
			UserListOrder::SubscriptionDateDesc,
			AppIDs::ConsumerAppId(GMOD_APP_ID),
			page,
		)
		.map_err(|error| format!("Could not load your subscriptions: {error}"))?
		.include_children(true)
		.allow_cached_response(0)
		.fetch(move |result| {
			let result = result.map_err(failed).and_then(|results| {
				let entries = (0..results.returned_results())
					.filter_map(|index| read_item(&results, index, true).transpose())
					.map(|details| details.map(|details| CollectionEntry::read(details.item.id, Some(&details))))
					.collect::<Result<Vec<_>, String>>()?;
				Ok((results.total_results(), entries))
			});
			let _ = sender.send((page, result));
		});
	Ok(())
}

/// Also returns how many subscriptions Steam counted but left out of the pages.
fn server_subscriptions(account: SteamId, progress: Option<&Channel<CollectionProgress>>) -> Result<(Vec<CollectionEntry>, usize), String> {
	let (sender, receiver) = mpsc::channel();
	let mut pending = HashMap::from([(1, Instant::now())]);
	request_page(account, 1, sender.clone())?;
	let mut next_page = 2;
	let mut pages = HashMap::new();
	let mut subscriptions = SubscriptionPages::default();
	while !pending.is_empty() {
		let remaining = pending.values().map(|started| STEAM_TIMEOUT.saturating_sub(started.elapsed())).min().unwrap();
		let (page, response) = receiver
			.recv_timeout(remaining)
			.map_err(|_| "Steam took too long to return your subscriptions. Try again.".to_string())?;
		if pending.remove(&page).is_none() {
			return Err("Steam returned an unexpected page of subscriptions. Try again.".into());
		}
		let (total, entries) = response?;
		let page_ids = entries.iter().map(|entry| entry.id.parse().map(PublishedFileId)).collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
		subscriptions.append(page, total, page_ids).map_err(|error| error.to_string())?;
		pages.insert(page, entries);
		if let Some(channel) = progress {
			report(channel, subscriptions.received(), total as usize);
		}
		while pending.len() < PAGES_IN_FLIGHT && next_page <= total.div_ceil(RESULTS_PER_PAGE as u32) {
			pending.insert(next_page, Instant::now());
			request_page(account, next_page, sender.clone())?;
			next_page += 1;
		}
	}
	if !subscriptions.complete() {
		return Err(INCOMPLETE.into());
	}
	let mut pages: Vec<_> = pages.into_iter().collect();
	pages.sort_by_key(|(page, _)| *page);
	Ok((pages.into_iter().flat_map(|(_, entries)| entries).collect(), subscriptions.missing()))
}

/// Items Steam reports locally or that the app subscribed to, but that the server query did not return yet.
fn local_only(server: &HashSet<String>, local: Vec<PublishedFileId>, session: &HashSet<PublishedFileId>) -> Vec<PublishedFileId> {
	let mut seen = HashSet::new();
	local
		.into_iter()
		.chain(session.iter().copied())
		.filter(|id| !server.contains(&id.0.to_string()) && seen.insert(*id))
		.collect()
}

/// Steam keeps reporting an item as subscribed for the rest of the session after the app unsubscribes it.
fn without_unsubscribed(entries: &mut Vec<CollectionEntry>, unsubscribed: &HashSet<PublishedFileId>) {
	let unsubscribed: HashSet<String> = unsubscribed.iter().map(|id| id.0.to_string()).collect();
	entries.retain(|entry| !unsubscribed.contains(&entry.id));
}

fn fetch(account: SteamId, progress: Option<&Channel<CollectionProgress>>) -> Result<SubscriptionSnapshot, String> {
	let fetched_at = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_err(|error| error.to_string())?.as_secs();
	let (mut items, missing) = server_subscriptions(account, progress)?;
	if steam!().client().steam_id != account {
		return Err("The Steam account changed. Try again.".into());
	}
	let (unsubscribed, subscribed) = game_addons!().session_subscriptions();
	let server: HashSet<String> = items.iter().map(|entry| entry.id.clone()).collect();
	let mut local = Vec::new();
	for chunk in local_only(&server, steam!().client().ugc().subscribed_items(true), &subscribed).chunks(RESULTS_PER_PAGE) {
		let results = query_items(chunk.to_vec(), true)?;
		local.extend(
			chunk
				.iter()
				.map(|id| CollectionEntry::read(*id, results.iter().flatten().find(|details| details.item.id == *id))),
		);
	}
	without_unsubscribed(&mut items, &unsubscribed);
	without_unsubscribed(&mut local, &unsubscribed);
	let unavailable = local.iter().filter(|entry| entry.status == EntryStatus::Unavailable).count();
	if !missing_explained(missing, unavailable) {
		return Err(INCOMPLETE.into());
	}
	items.extend(local);
	Ok(SubscriptionSnapshot { fetched_at, items })
}

fn snapshot(refresh: bool, progress: Option<&Channel<CollectionProgress>>) -> Result<SubscriptionSnapshot, String> {
	if collections::saving() {
		return Err("A collection is saving to Steam. Try again when it finishes.".into());
	}
	let _fetching = FETCH.lock();
	connected()?;
	let account = steam!().client().steam_id;
	if !refresh {
		if let Some(cached) = CACHE.lock().as_ref().filter(|cached| cached.account == account && cached.fetched.elapsed() < SNAPSHOT_MAX_AGE) {
			return Ok(cached.snapshot.clone());
		}
	}
	let generation = GENERATION.load(Ordering::Acquire);
	let snapshot = fetch(account, progress)?;
	let mut cache = CACHE.lock();
	if GENERATION.load(Ordering::Acquire) == generation {
		*cache = Some(Cached {
			account,
			fetched: Instant::now(),
			snapshot: snapshot.clone(),
		});
	}
	Ok(snapshot)
}

#[derive(Default, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncSkipped {
	unsupported: usize,
	extras: usize,
	unavailable: usize,
}

/// `add`: subscribed but not in the collection. `remove`: in the collection but not subscribed.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct SyncPlan {
	add: Vec<String>,
	remove: Vec<String>,
	both: Vec<String>,
	skipped: SyncSkipped,
}

fn compared(entry: &CollectionEntry, include_extras: bool) -> bool {
	include_extras || entry.kind == ItemKind::Addon
}

/// Keep in step with `planSync` in app/collections.js.
fn plan_sync(collection: &str, items: &[String], entries: &HashMap<&str, &CollectionEntry>, subscriptions: &[CollectionEntry], include_extras: bool) -> SyncPlan {
	let subscribed: HashMap<&str, &CollectionEntry> = subscriptions.iter().map(|entry| (entry.id.as_str(), entry)).collect();
	let members: HashSet<&str> = items.iter().map(String::as_str).collect();
	let mut plan = SyncPlan::default();
	for item in items {
		let subscription = subscribed.get(item.as_str()).copied();
		let Some(entry) = subscription.or_else(|| entries.get(item.as_str()).copied()) else { continue };
		if entry.status == EntryStatus::Unsupported {
			plan.skipped.unsupported += 1;
		} else if entry.status != EntryStatus::Unavailable && !compared(entry, include_extras) {
			plan.skipped.extras += 1;
		} else if subscription.is_some() {
			plan.both.push(item.clone());
		} else {
			plan.remove.push(item.clone());
		}
	}
	for entry in subscriptions.iter().filter(|entry| !members.contains(entry.id.as_str())) {
		if entry.id == collection || entry.status == EntryStatus::Unsupported {
			plan.skipped.unsupported += 1;
		} else if entry.status != EntryStatus::Available {
			plan.skipped.unavailable += 1;
		} else if !compared(entry, include_extras) {
			plan.skipped.extras += 1;
		} else {
			plan.add.push(entry.id.clone());
		}
	}
	plan
}

pub(crate) struct SubscriptionChanges {
	pub(crate) subscribe: Vec<PublishedFileId>,
	pub(crate) unsubscribe: Vec<PublishedFileId>,
	pub(crate) skipped: usize,
}

/// What would make the subscriptions match a collection as it is on Steam now.
pub(crate) fn subscription_changes(collection: PublishedFileId, include_extras: bool) -> Result<SubscriptionChanges, String> {
	let details = query_item(collection)?;
	if !details.gmod_collection {
		return Err(format!("{} is not a Garry's Mod collection.", details.item.title));
	}
	let items: Vec<String> = details.children.iter().map(|id| id.0.to_string()).collect();
	let snapshot = snapshot(false, None)?;
	let subscribed: HashSet<&str> = snapshot.items.iter().map(|entry| entry.id.as_str()).collect();
	let missing = items.iter().filter(|item| !subscribed.contains(item.as_str())).cloned().collect();
	let lookup = collections::lookup_entries(missing)?;
	let entries: HashMap<&str, &CollectionEntry> = lookup.entries.iter().map(|entry| (entry.id.as_str(), entry)).collect();
	let plan = plan_sync(&collection.0.to_string(), &items, &entries, &snapshot.items, include_extras);
	let parse = |ids: &[String]| ids.iter().filter_map(|id| id.parse().ok().map(PublishedFileId)).collect::<Vec<_>>();
	let available: Vec<String> = plan
		.remove
		.iter()
		.filter(|item| entries.get(item.as_str()).is_some_and(|entry| entry.status == EntryStatus::Available))
		.cloned()
		.collect();
	Ok(SubscriptionChanges {
		skipped: plan.remove.len() - available.len(),
		subscribe: parse(&available),
		unsubscribe: parse(&plan.add),
	})
}

#[tauri::command]
pub async fn subscription_snapshot(refresh: bool, on_progress: Channel<CollectionProgress>) -> Result<SubscriptionSnapshot, String> {
	let result = tauri::async_runtime::spawn_blocking(move || snapshot(refresh, Some(&on_progress)))
		.await
		.map_err(|error| error.to_string())?;
	match &result {
		Ok(snapshot) => println!("Compared {} subscriptions", snapshot.items.len()),
		Err(error) => eprintln!("Could not load subscriptions: {}", error),
	}
	result
}

/// Steam's local list, read without a network request, to catch subscriptions made since the snapshot.
#[tauri::command]
pub fn local_subscribed_ids() -> Result<Vec<String>, String> {
	connected()?;
	let (unsubscribed, _) = game_addons!().session_subscriptions();
	Ok(steam!()
		.client()
		.ugc()
		.subscribed_items(true)
		.into_iter()
		.filter(|id| !unsubscribed.contains(id))
		.map(|id| id.0.to_string())
		.collect())
}

#[cfg(test)]
mod tests {
	use super::*;
	use EntryStatus::*;
	use ItemKind::*;

	fn ids(values: &[&str]) -> Vec<String> {
		values.iter().map(|id| id.to_string()).collect()
	}

	fn plan(items: &[&str], known: &[CollectionEntry], subscriptions: &[CollectionEntry], include_extras: bool) -> SyncPlan {
		let entries = known.iter().map(|entry| (entry.id.as_str(), entry)).collect();
		plan_sync("1", &ids(items), &entries, subscriptions, include_extras)
	}

	#[test]
	fn sync_compares_addons_and_leaves_extras_unless_included() {
		let subscriptions = [
			CollectionEntry::test("10", Available, Addon),
			CollectionEntry::test("11", Available, Addon),
			CollectionEntry::test("12", Available, Dupe),
			CollectionEntry::test("13", Available, Save),
		];
		let known = [CollectionEntry::test("20", Available, Addon), CollectionEntry::test("21", Available, Demo)];
		let result = plan(&["10", "20", "21", "13"], &known, &subscriptions, false);
		assert_eq!(result.add, ids(&["11"]));
		assert_eq!(result.remove, ids(&["20"]));
		assert_eq!(result.both, ids(&["10"]));
		assert_eq!(result.skipped, SyncSkipped { unsupported: 0, extras: 3, unavailable: 0 });

		let result = plan(&["10", "20", "21", "13"], &known, &subscriptions, true);
		assert_eq!(result.add, ids(&["11", "12"]));
		assert_eq!(result.remove, ids(&["20", "21"]));
		assert_eq!(result.both, ids(&["10", "13"]));
		assert_eq!(result.skipped, SyncSkipped::default());
	}

	#[test]
	fn sync_never_touches_collections_or_other_games_and_keeps_unavailable_items_removable() {
		let subscriptions = [
			CollectionEntry::test("1", Unsupported, Other),
			CollectionEntry::test("30", Unsupported, Other),
			CollectionEntry::test("31", Unavailable, Other),
			CollectionEntry::test("32", Banned, Addon),
		];
		let known = [
			CollectionEntry::test("40", Unsupported, Other),
			CollectionEntry::test("41", Unavailable, Other),
			CollectionEntry::test("42", Banned, Addon),
			CollectionEntry::test("43", Banned, Dupe),
		];
		let result = plan(&["40", "41", "42", "43", "44", "30"], &known, &subscriptions, false);
		assert!(result.add.is_empty());
		assert_eq!(result.remove, ids(&["41", "42"]));
		assert!(result.both.is_empty());
		assert_eq!(result.skipped, SyncSkipped { unsupported: 3, extras: 1, unavailable: 2 });
	}

	#[test]
	fn session_overlay_hides_unsubscribed_items_and_adds_new_subscriptions() {
		let server: HashSet<String> = HashSet::from(["1".into(), "2".into()]);
		let session = HashSet::from([PublishedFileId(2), PublishedFileId(4)]);
		let mut extra = local_only(&server, vec![PublishedFileId(1), PublishedFileId(3), PublishedFileId(3)], &session);
		extra.sort();
		assert_eq!(extra, vec![PublishedFileId(3), PublishedFileId(4)]);

		let mut entries = vec![CollectionEntry::test("1", Available, Addon), CollectionEntry::test("2", Available, Addon)];
		without_unsubscribed(&mut entries, &HashSet::from([PublishedFileId(2)]));
		assert_eq!(entries.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(), ["1"]);
	}
}

#[tauri::command]
pub fn browse_subscribed_addons(page: u32) -> Option<(u32, Vec<Addon>)> {
	let results = Arc::new(Mutex::new(None));

	let results_ref = results.clone();
	let client = steam!().client();
	client
		.ugc()
		.query_user(
			client.steam_id.account_id(),
			steamworks::UserList::Subscribed,
			steamworks::UGCType::ItemsReadyToUse,
			steamworks::UserListOrder::SubscriptionDateDesc,
			steamworks::AppIDs::ConsumerAppId(GMOD_APP_ID),
			page,
		)
		.ok()?
		.require_tag("addon")
		.fetch(move |result: Result<QueryResults<'_>, SteamError>| {
			if let Ok(data) = result {
				*results_ref.lock() = Some(Some((
					data.total_results(),
					data.iter()
						.enumerate()
						.map(|(i, x)| {
							let mut item: WorkshopItem = x.unwrap().into();
							item.preview_url = data.preview_url(i as u32);
							item.read_statistics(&data, i as u32);
							item.subscriptions = data.statistic(i as u32, steamworks::UGCStatisticType::Subscriptions).unwrap_or(0);
							item.into()
						})
						.collect::<Vec<Addon>>(),
				)));
			} else {
				*results_ref.lock() = Some(None);
			}
		});

	mutex_wait!(results, {
		steam!().run_callbacks();
	});

	Arc::try_unwrap(results).unwrap().into_inner().unwrap()
}
