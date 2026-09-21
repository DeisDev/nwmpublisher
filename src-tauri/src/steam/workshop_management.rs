use std::{
	collections::HashSet,
	sync::mpsc,
	time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use steamworks::{AppIDs, FileType, PublishedFileId, QueryResult, QueryResults, SteamId, UGCQueryType, UGCType};

use super::workshop::{WorkshopItem, WorkshopVisibility};
use crate::GMOD_APP_ID;

const STEAM_TIMEOUT: Duration = Duration::from_secs(30);
static SAVE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredAddon {
	id: String,
	title: Option<String>,
	preview_url: Option<String>,
	banned: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopDetails {
	item: WorkshopItem,
	dependencies: Vec<RequiredAddon>,
	can_edit: bool,
}

#[derive(Serialize)]
pub struct AddonSearch {
	items: Vec<RequiredAddon>,
	total: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopSettingsRequest {
	addon_id: String,
	original_dependencies: Vec<String>,
	dependencies: Vec<String>,
	original_visibility: WorkshopVisibility,
	visibility: WorkshopVisibility,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopSaveResult {
	dependencies: Vec<String>,
	visibility: WorkshopVisibility,
	error: Option<String>,
}

fn dependency_changes(
	parent: PublishedFileId,
	original: &[String],
	desired: &[String],
) -> Result<(Vec<PublishedFileId>, Vec<PublishedFileId>), String> {
	let parse = |ids: &[String]| -> Result<Vec<PublishedFileId>, String> {
		let ids = ids.iter().map(|id| parse_id(id)).collect::<Result<Vec<_>, _>>()?;
		if ids.iter().collect::<HashSet<_>>().len() != ids.len() {
			return Err("A required addon appears more than once.".into());
		}
		Ok(ids)
	};
	let original = parse(original)?;
	let desired = parse(desired)?;
	if desired.contains(&parent) {
		return Err("An addon cannot require itself.".into());
	}
	let added = desired.iter().filter(|id| !original.contains(id)).copied().collect();
	let removed = original.iter().filter(|id| !desired.contains(id)).copied().collect();
	Ok((added, removed))
}

// steamworks 0.11 consumes even unregistered call results in its callback pump.
// Hold the dispatch lock until this raw result is read; other callbacks remain
// queued while Steam's periodic frame work continues, including on timeout.
fn change_dependency(parent: PublishedFileId, child: PublishedFileId, add: bool) -> Result<(), String> {
	use steamworks::sys;
	connected()?;
	let dispatch = steam!().callback_dispatch.lock();
	let _client = steam!().client();
	unsafe {
		let ugc = sys::SteamAPI_SteamUGC_v018();
		let utils = sys::SteamAPI_SteamUtils_v010();
		if ugc.is_null() || utils.is_null() {
			return Err("Steam Workshop is unavailable.".into());
		}
		let call = if add {
			sys::SteamAPI_ISteamUGC_AddDependency(ugc, parent.0, child.0)
		} else {
			sys::SteamAPI_ISteamUGC_RemoveDependency(ugc, parent.0, child.0)
		};
		if call == 0 {
			return Err("Steam did not start the required-addon change.".into());
		}
		let start = Instant::now();
		let mut failed = false;
		while !sys::SteamAPI_ISteamUtils_IsAPICallCompleted(utils, call, &mut failed) {
			if start.elapsed() >= STEAM_TIMEOUT {
				return Err("Steam timed out. The change may still complete; reload Workshop settings before retrying.".into());
			}
			sys::SteamAPI_ManualDispatch_RunFrame(sys::SteamAPI_GetHSteamPipe());
			std::thread::sleep(Duration::from_millis(20));
		}
		if failed {
			return Err(format!(
				"Steam connection failure: {:?}",
				sys::SteamAPI_ISteamUtils_GetAPICallFailureReason(utils, call)
			));
		}
		let (result, returned_parent, returned_child) = if add {
			let mut result = std::mem::MaybeUninit::<sys::AddUGCDependencyResult_t>::uninit();
			if !sys::SteamAPI_ISteamUtils_GetAPICallResult(
				utils,
				call,
				result.as_mut_ptr().cast(),
				std::mem::size_of::<sys::AddUGCDependencyResult_t>() as i32,
				sys::AddUGCDependencyResult_t_k_iCallback as i32,
				&mut failed,
			) || failed
			{
				return Err("Could not confirm the required-addon addition. Reload Workshop settings before retrying.".into());
			}
			let result = result.assume_init();
			(result.m_eResult, result.m_nPublishedFileId, result.m_nChildPublishedFileId)
		} else {
			let mut result = std::mem::MaybeUninit::<sys::RemoveUGCDependencyResult_t>::uninit();
			if !sys::SteamAPI_ISteamUtils_GetAPICallResult(
				utils,
				call,
				result.as_mut_ptr().cast(),
				std::mem::size_of::<sys::RemoveUGCDependencyResult_t>() as i32,
				sys::RemoveUGCDependencyResult_t_k_iCallback as i32,
				&mut failed,
			) || failed
			{
				return Err("Could not confirm the required-addon removal. Reload Workshop settings before retrying.".into());
			}
			let result = result.assume_init();
			(result.m_eResult, result.m_nPublishedFileId, result.m_nChildPublishedFileId)
		};
		if returned_parent != parent.0 || returned_child != child.0 {
			return Err("Steam returned a result for a different dependency.".into());
		}
		if result != sys::EResult::k_EResultOK {
			return Err(steamworks::SteamError::from(result).to_string());
		}
	}
	drop(dispatch);
	steam!().run_callbacks();
	Ok(())
}

fn change_visibility(id: PublishedFileId, visibility: WorkshopVisibility) -> Result<(), String> {
	connected()?;
	let visibility = match visibility {
		WorkshopVisibility::Public => steamworks::PublishedFileVisibility::Public,
		WorkshopVisibility::FriendsOnly => steamworks::PublishedFileVisibility::FriendsOnly,
		WorkshopVisibility::Private => steamworks::PublishedFileVisibility::Private,
		WorkshopVisibility::Unlisted => steamworks::PublishedFileVisibility::Unlisted,
	};
	let (sender, receiver) = mpsc::sync_channel(1);
	steam!()
		.client()
		.ugc()
		.start_item_update(GMOD_APP_ID, id)
		.visibility(visibility)
		.submit(None, move |result| {
			let _ = sender.send(result);
		});
	let (_, agreement) = receiver
		.recv_timeout(STEAM_TIMEOUT)
		.map_err(|error| format!("Could not confirm visibility: {error}. Reload Workshop settings before retrying."))?
		.map_err(|error| format!("Could not change visibility: {error}"))?;
	if agreement {
		crate::path::open("https://steamcommunity.com/workshop/workshoplegalagreement");
	}
	Ok(())
}

fn apply_settings_changes(
	mut result: WorkshopSaveResult,
	added: &[PublishedFileId],
	removed: &[PublishedFileId],
	visibility: Option<WorkshopVisibility>,
	mut dependency: impl FnMut(PublishedFileId, bool) -> Result<(), String>,
	mut set_visibility: impl FnMut(WorkshopVisibility) -> Result<(), String>,
) -> WorkshopSaveResult {
	for (children, add) in [(added, true), (removed, false)] {
		for child in children {
			if let Err(error) = dependency(*child, add) {
				result.error = Some(format!(
					"Could not {} required addon {}: {error}. Any confirmed earlier changes have been saved.",
					if add { "add" } else { "remove" },
					child.0
				));
				return result;
			}
			if add {
				result.dependencies.push(child.0.to_string());
			} else {
				result.dependencies.retain(|id| *id != child.0.to_string());
			}
		}
	}
	if let Some(visibility) = visibility {
		match set_visibility(visibility) {
			Ok(()) => result.visibility = visibility,
			Err(error) => result.error = Some(format!("{error}. Required-addon changes already confirmed by Steam have been saved.")),
		}
	}
	result
}

fn check_settings_conflict(request: &WorkshopSettingsRequest, current: &[PublishedFileId], visibility: WorkshopVisibility) -> Result<(), String> {
	let original = request
		.original_dependencies
		.iter()
		.map(|id| parse_id(id))
		.collect::<Result<HashSet<_>, _>>()?;
	let desired = request.dependencies.iter().map(|id| parse_id(id)).collect::<Result<HashSet<_>, _>>()?;
	if (original != desired && original != current.iter().copied().collect())
		|| (request.visibility != request.original_visibility && visibility != request.original_visibility)
	{
		return Err("Workshop settings changed on Steam while you were editing. Reload them and review your pending changes before saving.".into());
	}
	Ok(())
}

fn save_settings(request: WorkshopSettingsRequest) -> Result<WorkshopSaveResult, String> {
	let _saving = SAVE_LOCK.try_lock().ok_or("Another Workshop settings save is still running.")?;
	let id = parse_id(&request.addon_id)?;
	let (added, removed) = dependency_changes(id, &request.original_dependencies, &request.dependencies)?;
	let current = query_item(id)?;
	if !current.is_addon || current.owner != steam!().client().steam_id {
		return Err("Only the owner can edit this Garry's Mod addon's Workshop settings.".into());
	}
	let visibility = current.item.visibility.ok_or("Steam did not return the addon's visibility.")?;
	check_settings_conflict(&request, &current.children, visibility)?;
	// Validate every addition before making any changes. Unavailable existing
	// dependencies remain removable even when their details cannot be queried.
	for ids in added.chunks(steamworks::RESULTS_PER_PAGE as usize) {
		let results = query_items(ids.to_vec(), true)?;
		for child in ids {
			let entry = results
				.iter()
				.flatten()
				.find(|entry| entry.item.id == *child)
				.ok_or_else(|| format!("Required addon {} is unavailable.", child.0))?;
			if !entry.is_addon || entry.item.banned == Some(true) {
				return Err(format!("{} is not an available Garry's Mod addon.", child.0));
			}
			if entry.children.contains(&id) {
				return Err(format!(
					"{} already requires this addon; adding it would create a circular dependency.",
					entry.item.title
				));
			}
		}
	}
	let result = WorkshopSaveResult {
		dependencies: current.children.iter().map(|id| id.0.to_string()).collect(),
		visibility,
		error: None,
	};
	Ok(apply_settings_changes(
		result,
		&added,
		&removed,
		(request.visibility != request.original_visibility).then_some(request.visibility),
		|child, add| change_dependency(id, child, add),
		|visibility| change_visibility(id, visibility),
	))
}

struct ItemDetails {
	item: WorkshopItem,
	owner: SteamId,
	is_addon: bool,
	children: Vec<PublishedFileId>,
}

fn connected() -> Result<(), String> {
	if steam!().connected() {
		Ok(())
	} else {
		Err("Steam is offline. Connect Steam and try again.".into())
	}
}

fn is_gmod_addon(result: &QueryResult) -> bool {
	result.consumer_app_id == Some(GMOD_APP_ID)
		&& matches!(result.file_type, FileType::Community)
		&& result.tags.iter().any(|tag| tag.eq_ignore_ascii_case("addon"))
}

fn required_addon(item: &WorkshopItem) -> RequiredAddon {
	RequiredAddon {
		id: item.id.0.to_string(),
		title: Some(item.title.clone()),
		preview_url: item.preview_url.clone(),
		banned: item.banned,
	}
}

fn read_item(results: &QueryResults<'_>, index: u32, children: bool) -> Result<Option<ItemDetails>, String> {
	let Some(result) = results.get(index) else { return Ok(None) };
	let is_addon = is_gmod_addon(&result);
	let owner = result.owner;
	let children = if children && result.num_children > 0 {
		results
			.get_children(index)
			.ok_or_else(|| format!("Could not read required addons for {}.", result.published_file_id.0))?
	} else {
		Vec::new()
	};
	let mut item: WorkshopItem = result.into();
	item.preview_url = results.preview_url(index);
	item.subscriptions = results
		.statistic(index, steamworks::UGCStatisticType::Subscriptions)
		.ok_or_else(|| format!("Could not read subscribers for {}.", item.id.0))?;
	item.read_statistics(results, index);
	Ok(Some(ItemDetails {
		item,
		owner,
		is_addon,
		children,
	}))
}

fn query_items(ids: Vec<PublishedFileId>, children: bool) -> Result<Vec<Option<ItemDetails>>, String> {
	connected()?;
	let (sender, receiver) = mpsc::sync_channel(1);
	steam!()
		.client()
		.ugc()
		.query_items(ids)
		.map_err(|error| error.to_string())?
		.include_children(children)
		.include_long_desc(true)
		.allow_cached_response(0)
		.fetch(move |result| {
			let result = result.map_err(|error| format!("Workshop query failed: {error}")).and_then(|results| {
				(0..results.returned_results())
					.map(|index| read_item(&results, index, children))
					.collect()
			});
			let _ = sender.send(result);
		});
	receiver
		.recv_timeout(STEAM_TIMEOUT)
		.map_err(|error| format!("Waiting for Steam Workshop: {error}"))?
}

fn query_item(id: PublishedFileId) -> Result<ItemDetails, String> {
	query_items(vec![id], true)?
		.pop()
		.flatten()
		.ok_or_else(|| format!("Workshop item {} is unavailable. It may be private or deleted.", id.0))
}

fn load_details(id: PublishedFileId) -> Result<WorkshopDetails, String> {
	let parent = query_item(id)?;
	if !parent.is_addon {
		return Err("This item is not a Garry's Mod addon.".into());
	}
	let can_edit = parent.owner == steam!().client().steam_id;
	let mut dependencies = Vec::with_capacity(parent.children.len());
	for ids in parent.children.chunks(steamworks::RESULTS_PER_PAGE as usize) {
		let results = query_items(ids.to_vec(), false)?;
		for id in ids {
			dependencies.push(
				results
					.iter()
					.flatten()
					.find(|entry| entry.item.id == *id)
					.map(|entry| required_addon(&entry.item))
					.unwrap_or_else(|| RequiredAddon {
						id: id.0.to_string(),
						title: None,
						preview_url: None,
						banned: None,
					}),
			);
		}
	}
	Ok(WorkshopDetails {
		item: parent.item,
		dependencies,
		can_edit,
	})
}

fn parse_id(value: &str) -> Result<PublishedFileId, String> {
	if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
		return Err("Enter a valid Workshop item ID.".into());
	}
	value
		.parse::<u64>()
		.ok()
		.filter(|id| *id > 0)
		.map(PublishedFileId)
		.ok_or_else(|| "Enter a valid Workshop item ID.".into())
}

fn parse_reference(value: &str) -> Result<Option<PublishedFileId>, String> {
	let value = value.trim();
	if !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()) {
		return parse_id(value).map(Some);
	}
	if !(value.contains("://") || value.starts_with("steamcommunity.com/")) {
		return Ok(None);
	}
	let invalid = || "Use a steamcommunity.com Workshop item link or its numeric ID.".to_string();
	let value = value.strip_prefix("https://").or_else(|| value.strip_prefix("http://")).unwrap_or(value);
	let (host, path) = value.split_once('/').ok_or_else(invalid)?;
	if !host.eq_ignore_ascii_case("steamcommunity.com") {
		return Err(invalid());
	}
	let (path, query) = path.split('#').next().unwrap().split_once('?').ok_or_else(invalid)?;
	if !matches!(path.trim_end_matches('/'), "sharedfiles/filedetails" | "workshop/filedetails") {
		return Err(invalid());
	}
	let ids: Vec<_> = query.split('&').filter_map(|part| part.strip_prefix("id=")).collect();
	if ids.len() != 1 {
		return Err(invalid());
	}
	parse_id(ids[0]).map(Some)
}

fn search_addons(query: String, page: u32) -> Result<AddonSearch, String> {
	connected()?;
	let query = query.trim();
	if query.is_empty() || query.len() > 512 || query.contains('\0') || page == 0 {
		return Err("Enter an addon name, Workshop link, or item ID (up to 512 bytes).".into());
	}
	if let Some(id) = parse_reference(query)? {
		let details = query_item(id)?;
		if !details.is_addon {
			return Err("Choose an individual Garry's Mod addon, not a collection or an item for another game.".into());
		}
		return Ok(AddonSearch {
			items: vec![required_addon(&details.item)],
			total: 1,
		});
	}
	let (sender, receiver) = mpsc::sync_channel(1);
	steam!()
		.client()
		.ugc()
		.query_all(
			UGCQueryType::RankedByTextSearch,
			UGCType::ItemsReadyToUse,
			AppIDs::ConsumerAppId(GMOD_APP_ID),
			page,
		)
		.map_err(|error| error.to_string())?
		.require_tag("addon")
		.set_search_text(query)
		.fetch(move |result| {
			let result = result.map_err(|error| format!("Workshop search failed: {error}")).and_then(|results| {
				let mut items = Vec::new();
				for index in 0..results.returned_results() {
					let entry = read_item(&results, index, false)?
						.ok_or_else(|| "Steam returned an unavailable search result. Try the search again.".to_string())?;
					if entry.is_addon {
						items.push(required_addon(&entry.item));
					}
				}
				Ok(AddonSearch {
					items,
					total: results.total_results(),
				})
			});
			let _ = sender.send(result);
		});
	receiver
		.recv_timeout(STEAM_TIMEOUT)
		.map_err(|error| format!("Waiting for Workshop search: {error}"))?
}

#[tauri::command]
pub async fn workshop_details(addon_id: String) -> Result<WorkshopDetails, String> {
	let id = parse_id(&addon_id)?;
	tauri::async_runtime::spawn_blocking(move || load_details(id))
		.await
		.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn search_required_addons(query: String, page: u32) -> Result<AddonSearch, String> {
	tauri::async_runtime::spawn_blocking(move || search_addons(query, page))
		.await
		.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn save_workshop_settings(request: WorkshopSettingsRequest) -> Result<WorkshopSaveResult, String> {
	tauri::async_runtime::spawn_blocking(move || save_settings(request))
		.await
		.map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn workshop_references_accept_links_ids_and_searches() {
		for input in [
			"123",
			" 123 ",
			"https://steamcommunity.com/sharedfiles/filedetails/?id=123&searchtext=tools",
			"http://steamcommunity.com/workshop/filedetails/?id=123#comments",
			"steamcommunity.com/sharedfiles/filedetails?id=123",
		] {
			assert_eq!(parse_reference(input).unwrap(), Some(PublishedFileId(123)));
		}
		assert_eq!(parse_reference("wire tools").unwrap(), None);
		assert_eq!(parse_reference("18446744073709551615").unwrap(), Some(PublishedFileId(u64::MAX)));
	}

	#[test]
	fn workshop_references_reject_invalid_or_misleading_links() {
		for input in [
			"0",
			"18446744073709551616",
			"https://example.com/?id=123",
			"https://steamcommunity.com.evil/sharedfiles/filedetails/?id=123",
			"https://steamcommunity.com/sharedfiles/filedetails/?id=123&id=456",
			"https://steamcommunity.com/sharedfiles/filedetails/?id=-1",
			"https://steamcommunity.com/profiles/123",
			"https://steamcommunity.com/sharedfiles/filedetails/?id=abc",
		] {
			assert!(parse_reference(input).is_err(), "{input}");
		}
	}

	#[test]
	fn dependency_edits_only_change_added_and_removed_items() {
		let (added, removed) = dependency_changes(PublishedFileId(1), &["2".into(), "3".into()], &["3".into(), "4".into()]).unwrap();
		assert_eq!(added, vec![PublishedFileId(4)]);
		assert_eq!(removed, vec![PublishedFileId(2)]);
		assert_eq!(
			dependency_changes(PublishedFileId(1), &["2".into(), "3".into()], &["3".into(), "2".into()]).unwrap(),
			(vec![], vec![])
		);
	}

	#[test]
	fn dependency_edits_reject_self_duplicates_and_invalid_ids() {
		for ids in [vec!["1".into()], vec!["2".into(), "2".into()], vec!["0".into()], vec!["-2".into()]] {
			assert!(dependency_changes(PublishedFileId(1), &[], &ids).is_err());
		}
	}

	#[test]
	fn partial_dependency_failure_preserves_confirmed_changes_and_stops_saving() {
		let initial = WorkshopSaveResult {
			dependencies: vec!["2".into()],
			visibility: WorkshopVisibility::Private,
			error: None,
		};
		let mut calls = Vec::new();
		let result = apply_settings_changes(
			initial,
			&[PublishedFileId(3), PublishedFileId(4)],
			&[PublishedFileId(2)],
			Some(WorkshopVisibility::Public),
			|id, add| {
				calls.push((id.0, add));
				if id.0 == 4 {
					Err("Access denied".into())
				} else {
					Ok(())
				}
			},
			|_| panic!("Visibility must not be changed after a dependency failure"),
		);
		assert_eq!(calls, vec![(3, true), (4, true)]);
		assert_eq!(result.dependencies, vec!["2", "3"]);
		assert_eq!(result.visibility, WorkshopVisibility::Private);
		assert!(result.error.unwrap().contains("Access denied"));
		let (added, removed) = dependency_changes(PublishedFileId(1), &result.dependencies, &["3".into(), "4".into()]).unwrap();
		assert_eq!(added, vec![PublishedFileId(4)]);
		assert_eq!(removed, vec![PublishedFileId(2)]);
	}

	#[test]
	fn visibility_failure_does_not_lose_completed_dependency_changes() {
		let initial = WorkshopSaveResult {
			dependencies: vec!["2".into()],
			visibility: WorkshopVisibility::Private,
			error: None,
		};
		let result = apply_settings_changes(
			initial,
			&[PublishedFileId(3)],
			&[PublishedFileId(2)],
			Some(WorkshopVisibility::Unlisted),
			|_, _| Ok(()),
			|_| Err("Visibility rejected".into()),
		);
		assert_eq!(result.dependencies, vec!["3"]);
		assert_eq!(result.visibility, WorkshopVisibility::Private);
		assert!(result.error.unwrap().contains("Visibility rejected"));
	}

	#[test]
	fn successful_save_only_changes_requested_fields() {
		let initial = WorkshopSaveResult {
			dependencies: vec!["2".into()],
			visibility: WorkshopVisibility::FriendsOnly,
			error: None,
		};
		let result = apply_settings_changes(
			initial,
			&[],
			&[],
			Some(WorkshopVisibility::Unlisted),
			|_, _| panic!("Unchanged dependencies must not be written"),
			|_| Ok(()),
		);
		assert_eq!(result.dependencies, vec!["2"]);
		assert_eq!(result.visibility, WorkshopVisibility::Unlisted);
		assert!(result.error.is_none());
	}

	#[test]
	fn conflicts_protect_edited_fields_but_allow_unrelated_external_changes() {
		let mut request = WorkshopSettingsRequest {
			addon_id: "1".into(),
			original_dependencies: vec!["2".into()],
			dependencies: vec!["2".into(), "3".into()],
			original_visibility: WorkshopVisibility::Private,
			visibility: WorkshopVisibility::Private,
		};
		assert!(check_settings_conflict(&request, &[PublishedFileId(2), PublishedFileId(4)], WorkshopVisibility::Private).is_err());
		assert!(check_settings_conflict(&request, &[PublishedFileId(2)], WorkshopVisibility::Public).is_ok());
		request.dependencies = request.original_dependencies.clone();
		request.visibility = WorkshopVisibility::Unlisted;
		assert!(check_settings_conflict(&request, &[PublishedFileId(2)], WorkshopVisibility::Public).is_err());
		assert!(check_settings_conflict(&request, &[PublishedFileId(2), PublishedFileId(4)], WorkshopVisibility::Private).is_ok());
	}
}
