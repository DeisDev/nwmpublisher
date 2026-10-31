use std::{
	cell::Cell,
	collections::HashSet,
	path::{Path, PathBuf},
	sync::{
		atomic::{AtomicBool, Ordering},
		mpsc,
	},
	time::Duration,
};

use serde::{Deserialize, Serialize};
use steamworks::{AppIDs, FileType, ItemState, PublishedFileId, SteamError, UGCType, UserList, UserListOrder};
use tauri::ipc::Channel;

use super::{
	publishing::{PublishError, WorkshopIcon, WORKSHOP_DESCRIPTION_MAX_BYTES},
	translations::TITLE_MAX_BYTES,
	workshop::WorkshopVisibility,
	workshop_management::{
		change_dependency, change_subscription, connected, paced, parse_id, parse_reference, query_item, query_items, read_item, ItemDetails, Paced, SteamWriteError,
		WriteFailure, STEAM_TIMEOUT,
	},
	RESULTS_PER_PAGE,
};
use crate::GMOD_APP_ID;

const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const LEGAL_AGREEMENT: &str = "https://steamcommunity.com/workshop/workshoplegalagreement";
static CHANGE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
static CHANGE_STOP: AtomicBool = AtomicBool::new(false);
static SUBSCRIBE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// Collection writes hold Steam's callback dispatch for each round trip, so other Steam work would stall.
pub(crate) fn saving() -> bool {
	CHANGE_LOCK.is_locked()
}

pub(crate) fn subscribe_lock() -> Option<parking_lot::MutexGuard<'static, ()>> {
	SUBSCRIBE_LOCK.try_lock()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectionFields {
	title: String,
	description: String,
	visibility: WorkshopVisibility,
	items: Vec<String>,
}

impl CollectionFields {
	fn read(details: &ItemDetails) -> Result<Self, String> {
		Ok(CollectionFields {
			title: details.item.title.clone(),
			description: details.item.description.clone().unwrap_or_default(),
			visibility: details
				.item
				.visibility
				.ok_or_else(|| format!("Steam did not return the visibility of {}.", details.item.title))?,
			items: details.children.iter().map(|id| id.0.to_string()).collect(),
		})
	}
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
	id: String,
	fields: CollectionFields,
	preview_url: Option<String>,
	time_created: u32,
	time_updated: u32,
	views: Option<u64>,
	favorites: Option<u64>,
}

impl Collection {
	fn read(details: &ItemDetails) -> Result<Self, String> {
		Ok(Collection {
			id: details.item.id.0.to_string(),
			fields: CollectionFields::read(details)?,
			preview_url: details.item.preview_url.clone(),
			time_created: details.item.time_created,
			time_updated: details.item.time_updated,
			views: details.item.views,
			favorites: details.item.favorites,
		})
	}
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EntryStatus {
	Available,
	Unavailable,
	Banned,
	Unsupported,
}

/// Garry's Mod tags every Workshop item with its type: `addon`, `dupe`, `save` or `demo`.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemKind {
	Addon,
	Dupe,
	Save,
	Demo,
	Other,
}

impl ItemKind {
	fn of(tags: &[String]) -> Self {
		let tagged = |name: &str| tags.iter().any(|tag| tag.eq_ignore_ascii_case(name));
		[("addon", ItemKind::Addon), ("dupe", ItemKind::Dupe), ("save", ItemKind::Save), ("demo", ItemKind::Demo)]
			.into_iter()
			.find(|(name, _)| tagged(name))
			.map_or(ItemKind::Other, |(_, kind)| kind)
	}
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CollectionEntry {
	pub(crate) id: String,
	title: Option<String>,
	preview_url: Option<String>,
	file_size: Option<u64>,
	time_updated: Option<u32>,
	pub(crate) status: EntryStatus,
	pub(crate) kind: ItemKind,
	dependencies: Vec<String>,
}

impl CollectionEntry {
	pub(super) fn read(id: PublishedFileId, details: Option<&ItemDetails>) -> Self {
		let Some(details) = details else {
			return CollectionEntry {
				id: id.0.to_string(),
				title: None,
				preview_url: None,
				file_size: None,
				time_updated: None,
				status: EntryStatus::Unavailable,
				kind: ItemKind::Other,
				dependencies: Vec::new(),
			};
		};
		let status = if !details.gmod_item {
			EntryStatus::Unsupported
		} else if details.item.banned == Some(true) {
			EntryStatus::Banned
		} else {
			EntryStatus::Available
		};
		CollectionEntry {
			id: id.0.to_string(),
			title: Some(details.item.title.clone()),
			preview_url: details.item.preview_url.clone(),
			file_size: details.item.file_size,
			time_updated: Some(details.item.time_updated),
			status,
			kind: if details.gmod_item { ItemKind::of(&details.item.tags) } else { ItemKind::Other },
			// Only a Workshop item's children are its required addons; a nested collection's are its items.
			dependencies: if details.gmod_item {
				details.children.iter().map(|id| id.0.to_string()).collect()
			} else {
				Vec::new()
			},
		}
	}
}

#[cfg(test)]
impl CollectionEntry {
	pub(super) fn test(id: &str, status: EntryStatus, kind: ItemKind) -> Self {
		CollectionEntry {
			id: id.into(),
			title: None,
			preview_url: None,
			file_size: None,
			time_updated: None,
			status,
			kind,
			dependencies: Vec::new(),
		}
	}
}

#[derive(Serialize)]
pub struct EntryLookup {
	pub(super) entries: Vec<CollectionEntry>,
	invalid: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDraft {
	title: String,
	description: String,
	visibility: WorkshopVisibility,
	preview: Option<PathBuf>,
}

#[derive(Serialize)]
pub struct CreatedCollection {
	id: String,
	error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSaveRequest {
	collection_id: String,
	original: CollectionFields,
	desired: CollectionFields,
	preview: Option<PathBuf>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSaveResult {
	saved: CollectionFields,
	preview_saved: bool,
	stopped: bool,
	error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionChange {
	collection_id: String,
	#[serde(default)]
	add: Vec<String>,
	#[serde(default)]
	remove: Vec<String>,
}

#[derive(Serialize)]
pub struct CollectionOutcome {
	id: String,
	changed: Vec<String>,
	error: Option<String>,
}

/// Collections left after a stop or a Steam failure that ends the run have no outcome.
#[derive(Serialize)]
pub struct CollectionChanges {
	outcomes: Vec<CollectionOutcome>,
	stopped: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionProgress {
	completed: usize,
	total: usize,
	retry_in: Option<u64>,
}

#[derive(Serialize)]
pub struct ItemFailure {
	id: String,
	error: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeOutcome {
	subscribed: usize,
	already_subscribed: usize,
	skipped: usize,
	failures: Vec<ItemFailure>,
	error: Option<String>,
}

#[derive(Default)]
struct DetailsChange {
	title: Option<String>,
	description: Option<String>,
	visibility: Option<WorkshopVisibility>,
	preview: Option<PathBuf>,
}

impl DetailsChange {
	fn is_empty(&self) -> bool {
		self.title.is_none() && self.description.is_none() && self.visibility.is_none() && self.preview.is_none()
	}
}

fn find(results: &[Option<ItemDetails>], id: PublishedFileId) -> Option<&ItemDetails> {
	results.iter().flatten().find(|details| details.item.id == id)
}

fn unique_ids(ids: &[String]) -> Result<Vec<PublishedFileId>, String> {
	let mut seen = HashSet::new();
	let mut unique = Vec::with_capacity(ids.len());
	for id in ids {
		let id = parse_id(id)?;
		if seen.insert(id) {
			unique.push(id);
		}
	}
	Ok(unique)
}

pub(super) fn report(channel: &Channel<CollectionProgress>, completed: usize, total: usize) {
	send_progress(channel, completed, total, None);
}

fn report_retry(channel: &Channel<CollectionProgress>, completed: usize, total: usize, delay: Duration) {
	send_progress(channel, completed, total, Some(delay.as_secs()));
}

fn send_progress(channel: &Channel<CollectionProgress>, completed: usize, total: usize, retry_in: Option<u64>) {
	if let Err(error) = channel.send(CollectionProgress { completed, total, retry_in }) {
		eprintln!("Collection progress delivery failed: {error}");
	}
}

fn write_message(error: &SteamWriteError) -> String {
	match error.result {
		Some(SteamError::LimitExceeded) => "Steam refused more items in this collection.".into(),
		_ => error.to_string(),
	}
}

fn load_collections() -> Result<Vec<Collection>, String> {
	connected()?;
	let account = steam!().client().steam_id.account_id();
	let mut collections = Vec::new();
	for page in 1u32.. {
		let (sender, receiver) = mpsc::sync_channel(1);
		steam!()
			.client()
			.ugc()
			.query_user(
				account,
				UserList::Published,
				UGCType::Collections,
				UserListOrder::LastUpdatedDesc,
				AppIDs::ConsumerAppId(GMOD_APP_ID),
				page,
			)
			.map_err(|error| format!("Could not load your collections: {error}"))?
			.include_children(true)
			.include_long_desc(true)
			.allow_cached_response(0)
			.fetch(move |result| {
				let result = result.map_err(|error| format!("Could not load your collections: {error}")).and_then(|results| {
					let collections = (0..results.returned_results())
						.map(|index| {
							let details = read_item(&results, index, true)?
								.ok_or_else(|| "Steam returned an unavailable collection. Try loading again.".to_string())?;
							Collection::read(&details)
						})
						.collect::<Result<Vec<_>, String>>()?;
					Ok((results.total_results(), collections))
				});
				let _ = sender.send(result);
			});
		let (total, loaded) = receiver
			.recv_timeout(STEAM_TIMEOUT)
			.map_err(|error| format!("Waiting for Steam Workshop: {error}"))??;
		let empty = loaded.is_empty();
		collections.extend(loaded);
		if collections.len() >= total as usize {
			break;
		}
		if empty {
			return Err("Steam returned an empty page before all collections were loaded. Try loading again.".into());
		}
	}
	Ok(collections)
}

pub(super) fn lookup_entries(references: Vec<String>) -> Result<EntryLookup, String> {
	let mut ids = Vec::new();
	let mut seen = HashSet::new();
	let mut invalid = Vec::new();
	for reference in references {
		match parse_reference(&reference) {
			Ok(Some(id)) => {
				if seen.insert(id) {
					ids.push(id);
				}
			}
			_ => invalid.push(reference),
		}
	}
	let mut entries = Vec::with_capacity(ids.len());
	for chunk in ids.chunks(RESULTS_PER_PAGE) {
		let results = query_items(chunk.to_vec(), true)?;
		entries.extend(chunk.iter().map(|id| CollectionEntry::read(*id, find(&results, *id))));
	}
	Ok(EntryLookup { entries, invalid })
}

fn owned_collection(id: PublishedFileId) -> Result<ItemDetails, String> {
	let details = query_item(id)?;
	if !details.gmod_collection || details.owner != steam!().client().steam_id {
		return Err(format!("{} is not one of your Garry's Mod collections.", details.item.title));
	}
	Ok(details)
}

fn validate_details(title: &str, description: &str) -> Result<(), String> {
	if title.trim().is_empty() {
		return Err("Enter a title for the collection.".into());
	}
	if title.len() > TITLE_MAX_BYTES {
		return Err(format!("The title must be at most {TITLE_MAX_BYTES} UTF-8 bytes."));
	}
	if description.len() > WORKSHOP_DESCRIPTION_MAX_BYTES {
		return Err(format!("The description must be at most {WORKSHOP_DESCRIPTION_MAX_BYTES} UTF-8 bytes."));
	}
	if title.contains('\0') || description.contains('\0') {
		return Err("Remove the NUL character from the title or description.".into());
	}
	Ok(())
}

fn validate_preview(path: &Path) -> Result<(), String> {
	if !path.is_absolute() {
		return Err("Choose the image again.".into());
	}
	WorkshopIcon::new(path, false).map(drop).map_err(|error| match error {
		PublishError::IconTooLarge => "The image must be 1 MB or smaller.".into(),
		PublishError::IconTooSmall => "The image file is too small.".into(),
		PublishError::IconInvalidFormat => "The image must be a JPG, PNG, or GIF.".into(),
		PublishError::IOError(error) => error.to_string(),
		PublishError::ImageError { source, .. } => format!("Could not read the image: {source}"),
		error => error.to_string(),
	})
}

fn validate_additions(ids: &[PublishedFileId]) -> Result<(), String> {
	for chunk in ids.chunks(RESULTS_PER_PAGE) {
		let results = query_items(chunk.to_vec(), false)?;
		for id in chunk {
			let details = find(&results, *id).ok_or_else(|| format!("Workshop item {} is unavailable. It may be private or deleted.", id.0))?;
			if !details.gmod_item || details.item.banned == Some(true) {
				return Err(format!("{} is not an available Garry's Mod Workshop item.", details.item.title));
			}
		}
	}
	Ok(())
}

fn item_changes(collection: PublishedFileId, original: &[String], desired: &[String]) -> Result<(Vec<PublishedFileId>, Vec<PublishedFileId>), String> {
	let parse = |ids: &[String]| -> Result<Vec<PublishedFileId>, String> {
		let ids = ids.iter().map(|id| parse_id(id)).collect::<Result<Vec<_>, _>>()?;
		if ids.iter().collect::<HashSet<_>>().len() != ids.len() {
			return Err("An item appears more than once in the collection.".into());
		}
		Ok(ids)
	};
	let original = parse(original)?;
	let desired = parse(desired)?;
	if desired.contains(&collection) {
		return Err("A collection cannot contain itself.".into());
	}
	let added = desired.iter().filter(|id| !original.contains(id)).copied().collect();
	let removed = original.iter().filter(|id| !desired.contains(id)).copied().collect();
	Ok((added, removed))
}

fn check_conflict(original: &CollectionFields, desired: &CollectionFields, current: &CollectionFields) -> Result<(), String> {
	let items = |fields: &CollectionFields| fields.items.iter().cloned().collect::<HashSet<_>>();
	if (original.title != desired.title && current.title != original.title)
		|| (original.description != desired.description && current.description != original.description)
		|| (original.visibility != desired.visibility && current.visibility != original.visibility)
		|| (items(original) != items(desired) && items(current) != items(original))
	{
		return Err("This collection changed on Steam while you were editing. Reload it and review your changes before saving.".into());
	}
	Ok(())
}

fn update_details(id: PublishedFileId, change: &DetailsChange) -> Result<(), String> {
	connected()?;
	let mut update = steam!().client().ugc().start_item_update(GMOD_APP_ID, id);
	if let Some(title) = &change.title {
		update = update.title(title);
	}
	if let Some(description) = &change.description {
		update = update.description(description);
	}
	if let Some(visibility) = change.visibility {
		update = update.visibility(visibility.into());
	}
	if let Some(preview) = &change.preview {
		update = update.preview_path(preview);
	}
	let (sender, receiver) = mpsc::sync_channel(1);
	update.submit(None, move |result| {
		let _ = sender.send(result);
	});
	let (_, agreement) = receiver
		.recv_timeout(if change.preview.is_some() { UPLOAD_TIMEOUT } else { STEAM_TIMEOUT })
		.map_err(|_| "Steam timed out. The change may still complete; reload before retrying.".to_string())?
		.map_err(|error| error.to_string())?;
	if agreement {
		crate::path::open(LEGAL_AGREEMENT);
	}
	Ok(())
}

fn apply_save(
	mut saved: CollectionFields,
	added: &[PublishedFileId],
	removed: &[PublishedFileId],
	change: DetailsChange,
	mut change_item: impl FnMut(PublishedFileId, bool) -> Result<Paced, String>,
	mut change_details: impl FnMut(&DetailsChange) -> Result<(), String>,
	mut progress: impl FnMut(usize, usize),
) -> CollectionSaveResult {
	let total = added.len() + removed.len();
	let mut completed = 0;
	for (items, add) in [(added, true), (removed, false)] {
		for item in items {
			match change_item(*item, add) {
				Ok(Paced::Written) => {}
				Ok(Paced::Stopped) => {
					return CollectionSaveResult {
						saved,
						preview_saved: false,
						stopped: true,
						error: None,
					}
				}
				Err(error) => {
					return CollectionSaveResult {
						saved,
						preview_saved: false,
						stopped: false,
						error: Some(format!(
							"Could not {} item {}: {error}. Changes Steam already confirmed have been saved.",
							if add { "add" } else { "remove" },
							item.0
						)),
					}
				}
			}
			if add {
				saved.items.push(item.0.to_string());
			} else {
				saved.items.retain(|id| *id != item.0.to_string());
			}
			completed += 1;
			progress(completed, total);
		}
	}
	if change.is_empty() {
		return CollectionSaveResult {
			saved,
			preview_saved: false,
			stopped: false,
			error: None,
		};
	}
	if let Err(error) = change_details(&change) {
		return CollectionSaveResult {
			saved,
			preview_saved: false,
			stopped: false,
			error: Some(format!("Could not save the collection details: {error}. Item changes Steam already confirmed have been saved.")),
		};
	}
	let preview_saved = change.preview.is_some();
	if let Some(title) = change.title {
		saved.title = title;
	}
	if let Some(description) = change.description {
		saved.description = description;
	}
	if let Some(visibility) = change.visibility {
		saved.visibility = visibility;
	}
	CollectionSaveResult {
		saved,
		preview_saved,
		stopped: false,
		error: None,
	}
}

fn save(request: CollectionSaveRequest, on_progress: &Channel<CollectionProgress>) -> Result<CollectionSaveResult, String> {
	let _changing = CHANGE_LOCK.try_lock().ok_or("Another collection change is still running.")?;
	CHANGE_STOP.store(false, Ordering::Release);
	let id = parse_id(&request.collection_id)?;
	let CollectionSaveRequest { original, desired, preview, .. } = request;
	let (added, removed) = item_changes(id, &original.items, &desired.items)?;
	validate_details(&desired.title, &desired.description)?;
	if let Some(preview) = &preview {
		validate_preview(preview)?;
	}
	let current = owned_collection(id)?;
	let current = CollectionFields::read(&current)?;
	check_conflict(&original, &desired, &current)?;
	validate_additions(&added)?;
	let change = DetailsChange {
		title: (desired.title != original.title).then(|| desired.title.clone()),
		description: (desired.description != original.description).then(|| desired.description.clone()),
		visibility: (desired.visibility != original.visibility).then_some(desired.visibility),
		preview,
	};
	let total = added.len() + removed.len();
	let completed = Cell::new(0);
	report(on_progress, 0, total);
	Ok(apply_save(
		current,
		&added,
		&removed,
		change,
		|item, add| {
			paced(&CHANGE_STOP, || change_dependency(id, item, add), |delay| report_retry(on_progress, completed.get(), total, delay))
				.map_err(|error| write_message(&error))
		},
		|change| update_details(id, change),
		|done, total| {
			completed.set(done);
			report(on_progress, done, total);
		},
	))
}

fn create(draft: CollectionDraft) -> Result<CreatedCollection, String> {
	let _changing = CHANGE_LOCK.try_lock().ok_or("Another collection change is still running.")?;
	validate_details(&draft.title, &draft.description)?;
	if let Some(preview) = &draft.preview {
		validate_preview(preview)?;
	}
	connected()?;
	let (sender, receiver) = mpsc::sync_channel(1);
	steam!().client().ugc().create_item(GMOD_APP_ID, FileType::Collection, move |result| {
		let _ = sender.send(result);
	});
	let (id, agreement) = receiver
		.recv_timeout(STEAM_TIMEOUT)
		.map_err(|_| "Steam timed out. The collection may still have been created; reload your collections before trying again.".to_string())?
		.map_err(|error| format!("Steam could not create the collection: {error}"))?;
	if agreement {
		crate::path::open(LEGAL_AGREEMENT);
	}
	let change = DetailsChange {
		title: Some(draft.title),
		description: Some(draft.description),
		visibility: Some(draft.visibility),
		preview: draft.preview,
	};
	let error = update_details(id, &change)
		.err()
		.map(|error| format!("The collection was created, but its details could not be saved: {error}"));
	Ok(CreatedCollection { id: id.0.to_string(), error })
}

struct ParsedChange {
	collection: PublishedFileId,
	add: Vec<PublishedFileId>,
	remove: Vec<PublishedFileId>,
}

fn parse_changes(changes: Vec<CollectionChange>) -> Result<Vec<ParsedChange>, String> {
	let mut seen = HashSet::new();
	let mut parsed = Vec::with_capacity(changes.len());
	for change in changes {
		let collection = parse_id(&change.collection_id)?;
		if !seen.insert(collection) {
			return Err("A collection appears more than once.".into());
		}
		let add = unique_ids(&change.add)?;
		let remove = unique_ids(&change.remove)?;
		if add.contains(&collection) {
			return Err("A collection cannot contain itself.".into());
		}
		if add.iter().any(|item| remove.contains(item)) {
			return Err("An item cannot be added and removed at once.".into());
		}
		parsed.push(ParsedChange { collection, add, remove });
	}
	if parsed.iter().all(|change| change.add.is_empty() && change.remove.is_empty()) {
		return Err("Choose at least one collection and one item.".into());
	}
	Ok(parsed)
}

enum Flow {
	Continue,
	Stopped,
	Halted,
}

fn apply_change(
	change: &ParsedChange,
	children: &[PublishedFileId],
	mut write: impl FnMut(PublishedFileId, bool) -> Result<Paced, SteamWriteError>,
	mut step: impl FnMut(),
) -> (CollectionOutcome, Flow) {
	let mut outcome = CollectionOutcome {
		id: change.collection.0.to_string(),
		changed: Vec::new(),
		error: None,
	};
	for (items, add) in [(&change.add, true), (&change.remove, false)] {
		for item in items {
			if children.contains(item) != add {
				match write(*item, add) {
					Ok(Paced::Written) => outcome.changed.push(item.0.to_string()),
					Ok(Paced::Stopped) => return (outcome, Flow::Stopped),
					Err(error) => {
						outcome.error = Some(format!("Could not {} item {}: {}", if add { "add" } else { "remove" }, item.0, write_message(&error)));
						let flow = if error.kind() == WriteFailure::Stop { Flow::Halted } else { Flow::Continue };
						return (outcome, flow);
					}
				}
			}
			step();
		}
	}
	(outcome, Flow::Continue)
}

fn apply_changes(changes: Vec<CollectionChange>, on_progress: &Channel<CollectionProgress>) -> Result<CollectionChanges, String> {
	let _changing = CHANGE_LOCK.try_lock().ok_or("Another collection change is still running.")?;
	CHANGE_STOP.store(false, Ordering::Release);
	let changes = parse_changes(changes)?;
	let additions: HashSet<_> = changes.iter().flat_map(|change| change.add.iter().copied()).collect();
	validate_additions(&additions.into_iter().collect::<Vec<_>>())?;
	let total = changes.iter().map(|change| change.add.len() + change.remove.len()).sum();
	let completed = Cell::new(0);
	report(on_progress, 0, total);
	let mut result = CollectionChanges {
		outcomes: Vec::with_capacity(changes.len()),
		stopped: false,
	};
	for change in &changes {
		let start = completed.get();
		let (outcome, flow) = match owned_collection(change.collection) {
			Err(error) => (
				CollectionOutcome {
					id: change.collection.0.to_string(),
					changed: Vec::new(),
					error: Some(error),
				},
				Flow::Continue,
			),
			Ok(current) => apply_change(
				change,
				&current.children,
				|item, add| paced(&CHANGE_STOP, || change_dependency(change.collection, item, add), |delay| report_retry(on_progress, completed.get(), total, delay)),
				|| {
					completed.set(completed.get() + 1);
					report(on_progress, completed.get(), total);
				},
			),
		};
		result.outcomes.push(outcome);
		match flow {
			Flow::Continue => {
				completed.set(start + change.add.len() + change.remove.len());
				report(on_progress, completed.get(), total);
			}
			Flow::Stopped => {
				result.stopped = true;
				break;
			}
			Flow::Halted => break,
		}
	}
	Ok(result)
}

fn set_visibility(collections: Vec<String>, visibility: WorkshopVisibility) -> Result<Vec<CollectionOutcome>, String> {
	let _changing = CHANGE_LOCK.try_lock().ok_or("Another collection change is still running.")?;
	let change = DetailsChange {
		visibility: Some(visibility),
		..Default::default()
	};
	Ok(unique_ids(&collections)?
		.into_iter()
		.map(|collection| CollectionOutcome {
			id: collection.0.to_string(),
			changed: Vec::new(),
			error: owned_collection(collection).and_then(|_| update_details(collection, &change)).err(),
		})
		.collect())
}

fn delete_collection(id: PublishedFileId) -> Result<(), String> {
	owned_collection(id)?;
	let (sender, receiver) = mpsc::sync_channel(1);
	steam!().client().ugc().delete_item(id, move |result| {
		let _ = sender.send(result);
	});
	receiver
		.recv_timeout(STEAM_TIMEOUT)
		.map_err(|_| "Steam timed out. The collection may still be deleted; reload before retrying.".to_string())?
		.map_err(|error| error.to_string())
}

fn delete(collections: Vec<String>) -> Result<Vec<CollectionOutcome>, String> {
	let _changing = CHANGE_LOCK.try_lock().ok_or("Another collection change is still running.")?;
	let outcomes: Vec<_> = unique_ids(&collections)?
		.into_iter()
		.map(|collection| CollectionOutcome {
			id: collection.0.to_string(),
			changed: Vec::new(),
			error: delete_collection(collection).err(),
		})
		.collect();
	let deleted: HashSet<&str> = outcomes.iter().filter(|outcome| outcome.error.is_none()).map(|outcome| outcome.id.as_str()).collect();
	if app_data!().settings.read().collections_mirrors.iter().any(|id| deleted.contains(id.as_str())) {
		match crate::appdata::change_settings(|settings| {
			settings.collections_mirrors.retain(|id| !deleted.contains(id.as_str()));
			Ok(())
		}) {
			Ok(()) => app_data!().send(),
			Err(error) => wprintln!("Could not forget the mirror marks of deleted collections: {}", error),
		}
	}
	Ok(outcomes)
}

fn subscribe(collection: PublishedFileId, on_progress: &Channel<CollectionProgress>) -> Result<SubscribeOutcome, String> {
	let _subscribing = SUBSCRIBE_LOCK.try_lock().ok_or("Already subscribing to a collection.")?;
	let details = query_item(collection)?;
	if !details.gmod_collection {
		return Err(format!("{} is not a Garry's Mod collection.", details.item.title));
	}
	let mut items = Vec::with_capacity(details.children.len());
	let mut skipped = 0;
	for chunk in details.children.chunks(RESULTS_PER_PAGE) {
		let results = query_items(chunk.to_vec(), false)?;
		for id in chunk {
			match find(&results, *id) {
				Some(item) if item.gmod_item && item.item.banned != Some(true) => items.push(*id),
				_ => skipped += 1,
			}
		}
	}
	let mut outcome = SubscribeOutcome {
		subscribed: 0,
		already_subscribed: 0,
		skipped,
		failures: Vec::new(),
		error: None,
	};
	let total = items.len();
	let never = AtomicBool::new(false);
	for (index, id) in items.into_iter().enumerate() {
		if !steam!().connected() {
			outcome.error = Some("Steam went offline. Connect Steam and try again.".into());
			break;
		}
		if steam!().client().ugc().item_state(id).contains(ItemState::SUBSCRIBED) && !game_addons!().unsubscribed_this_session(id) {
			outcome.already_subscribed += 1;
		} else {
			match paced(&never, || change_subscription(id, true), |delay| report_retry(on_progress, index, total, delay)) {
				Ok(_) => {
					outcome.subscribed += 1;
					game_addons!().note_subscribed(id);
				}
				Err(error) if error.kind() == WriteFailure::Stop => {
					outcome.error = Some(error.to_string());
					break;
				}
				Err(error) => outcome.failures.push(ItemFailure {
					id: id.0.to_string(),
					error: error.to_string(),
				}),
			}
		}
		report(on_progress, index + 1, total);
	}
	Ok(outcome)
}

async fn blocking<T: Send + 'static>(task: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
	tauri::async_runtime::spawn_blocking(task).await.map_err(|error| error.to_string())?
}

fn log_outcomes(action: &str, outcomes: Result<&[CollectionOutcome], &String>) {
	match outcomes {
		Ok(outcomes) => {
			for outcome in outcomes {
				match &outcome.error {
					Some(error) => eprintln!("Could not {action} collection {}: {error}", outcome.id),
					None => println!("Collection {}: {action} done", outcome.id),
				}
			}
		}
		Err(error) => eprintln!("Could not {action} collections: {error}"),
	}
}

#[tauri::command]
pub async fn my_collections() -> Result<Vec<Collection>, String> {
	blocking(load_collections).await
}

#[tauri::command]
pub async fn collection_entries(references: Vec<String>) -> Result<EntryLookup, String> {
	blocking(move || lookup_entries(references)).await
}

#[tauri::command]
pub async fn create_collection(draft: CollectionDraft) -> Result<CreatedCollection, String> {
	let result = blocking(move || create(draft)).await;
	match &result {
		Ok(CreatedCollection { id, error: Some(error) }) => wprintln!("Created collection {} but could not save its details: {}", id, error),
		Ok(CreatedCollection { id, .. }) => println!("Created collection {}", id),
		Err(error) => eprintln!("Could not create a collection: {}", error),
	}
	result
}

#[tauri::command]
pub async fn save_collection(request: CollectionSaveRequest, on_progress: Channel<CollectionProgress>) -> Result<CollectionSaveResult, String> {
	let collection = request.collection_id.clone();
	let result = blocking(move || save(request, &on_progress)).await;
	match &result {
		Ok(CollectionSaveResult { error: Some(error), .. }) => wprintln!("Collection {} was only partly saved: {}", collection, error),
		Ok(CollectionSaveResult { stopped: true, .. }) => println!("Stopped saving collection {}", collection),
		Ok(_) => println!("Saved collection {}", collection),
		Err(error) => eprintln!("Could not save collection {}: {}", collection, error),
	}
	result
}

#[tauri::command]
pub fn stop_collection_change() {
	CHANGE_STOP.store(true, Ordering::Release);
}

#[tauri::command]
pub async fn apply_collection_changes(changes: Vec<CollectionChange>, on_progress: Channel<CollectionProgress>) -> Result<CollectionChanges, String> {
	let result = blocking(move || apply_changes(changes, &on_progress)).await;
	log_outcomes("change items in", result.as_ref().map(|changes| changes.outcomes.as_slice()));
	if let Ok(CollectionChanges { stopped: true, .. }) = &result {
		println!("Stopped changing collection items");
	}
	result
}

#[tauri::command]
pub async fn set_collections_visibility(collections: Vec<String>, visibility: WorkshopVisibility) -> Result<Vec<CollectionOutcome>, String> {
	let result = blocking(move || set_visibility(collections, visibility)).await;
	log_outcomes("change the visibility of", result.as_deref());
	result
}

#[tauri::command]
pub async fn delete_collections(collections: Vec<String>) -> Result<Vec<CollectionOutcome>, String> {
	let result = blocking(move || delete(collections)).await;
	log_outcomes("delete", result.as_deref());
	result
}

#[tauri::command]
pub async fn subscribe_collection(collection_id: String, on_progress: Channel<CollectionProgress>) -> Result<SubscribeOutcome, String> {
	let id = parse_id(&collection_id)?;
	let result = blocking(move || subscribe(id, &on_progress)).await;
	match &result {
		Ok(outcome) => println!(
			"Subscribed to {} items from collection {} ({} already subscribed, {} skipped, {} failed)",
			outcome.subscribed,
			collection_id,
			outcome.already_subscribed,
			outcome.skipped,
			outcome.failures.len()
		),
		Err(error) => eprintln!("Could not subscribe to collection {}: {}", collection_id, error),
	}
	result
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fields(items: &[&str]) -> CollectionFields {
		CollectionFields {
			title: "Server pack".into(),
			description: "Everything the server needs".into(),
			visibility: WorkshopVisibility::Private,
			items: items.iter().map(|id| id.to_string()).collect(),
		}
	}

	#[test]
	fn item_edits_only_change_added_and_removed_items() {
		let (added, removed) = item_changes(PublishedFileId(1), &["2".into(), "3".into()], &["3".into(), "4".into()]).unwrap();
		assert_eq!(added, vec![PublishedFileId(4)]);
		assert_eq!(removed, vec![PublishedFileId(2)]);
		assert_eq!(item_changes(PublishedFileId(1), &["2".into(), "3".into()], &["3".into(), "2".into()]).unwrap(), (vec![], vec![]));
	}

	#[test]
	fn item_edits_reject_self_duplicates_and_invalid_ids() {
		for ids in [vec!["1".into()], vec!["2".into(), "2".into()], vec!["0".into()], vec!["abc".into()]] {
			assert!(item_changes(PublishedFileId(1), &[], &ids).is_err());
		}
	}

	#[test]
	fn details_must_fit_steam_limits() {
		assert!(validate_details("Pack", "").is_ok());
		assert!(validate_details("  ", "").is_err());
		assert!(validate_details(&"a".repeat(TITLE_MAX_BYTES + 1), "").is_err());
		assert!(validate_details("Pack", &"a".repeat(WORKSHOP_DESCRIPTION_MAX_BYTES + 1)).is_err());
		assert!(validate_details("Pa\0ck", "").is_err());
	}

	#[test]
	fn conflicts_protect_edited_fields_but_allow_unrelated_external_changes() {
		let original = fields(&["2"]);
		let mut desired = fields(&["2", "3"]);
		let mut current = fields(&["2", "4"]);
		assert!(check_conflict(&original, &desired, &current).is_err());
		current.items = original.items.clone();
		current.title = "Renamed on Steam".into();
		assert!(check_conflict(&original, &desired, &current).is_ok());
		desired.title = "Renamed here".into();
		assert!(check_conflict(&original, &desired, &current).is_err());
		desired.title = original.title.clone();
		desired.visibility = WorkshopVisibility::Public;
		current.visibility = WorkshopVisibility::Unlisted;
		assert!(check_conflict(&original, &desired, &current).is_err());
	}

	#[test]
	fn partial_item_failure_keeps_confirmed_changes_and_skips_details() {
		let mut calls = Vec::new();
		let change = DetailsChange {
			title: Some("New title".into()),
			..Default::default()
		};
		let result = apply_save(
			fields(&["2"]),
			&[PublishedFileId(3), PublishedFileId(4)],
			&[PublishedFileId(2)],
			change,
			|id, add| {
				calls.push((id.0, add));
				if id.0 == 4 {
					Err("Access denied".into())
				} else {
					Ok(Paced::Written)
				}
			},
			|_| panic!("Details must not be changed after an item failure"),
			|_, _| {},
		);
		assert!(!result.stopped);
		assert_eq!(calls, vec![(3, true), (4, true)]);
		assert_eq!(result.saved.items, vec!["2", "3"]);
		assert_eq!(result.saved.title, "Server pack");
		assert!(result.error.unwrap().contains("Access denied"));
	}

	#[test]
	fn details_failure_keeps_confirmed_item_changes() {
		let change = DetailsChange {
			visibility: Some(WorkshopVisibility::Public),
			preview: Some(PathBuf::from("preview.png")),
			..Default::default()
		};
		let result = apply_save(
			fields(&["2"]),
			&[PublishedFileId(3)],
			&[PublishedFileId(2)],
			change,
			|_, _| Ok(Paced::Written),
			|_| Err("Rejected".into()),
			|_, _| {},
		);
		assert_eq!(result.saved.items, vec!["3"]);
		assert_eq!(result.saved.visibility, WorkshopVisibility::Private);
		assert!(!result.preview_saved);
		assert!(result.error.unwrap().contains("Rejected"));
	}

	#[test]
	fn successful_save_only_changes_requested_fields() {
		let change = DetailsChange {
			description: Some("Updated".into()),
			preview: Some(PathBuf::from("preview.png")),
			..Default::default()
		};
		let result = apply_save(fields(&["2"]), &[], &[], change, |_, _| panic!("Unchanged items must not be written"), |_| Ok(()), |_, _| {});
		assert_eq!(result.saved.items, vec!["2"]);
		assert_eq!(result.saved.description, "Updated");
		assert_eq!(result.saved.title, "Server pack");
		assert!(result.preview_saved);
		assert!(result.error.is_none());
		let result = apply_save(fields(&[]), &[], &[], DetailsChange::default(), |_, _| unreachable!(), |_| panic!("Nothing to save"), |_, _| {});
		assert!(result.error.is_none());
	}

	#[test]
	fn stopped_save_keeps_confirmed_changes_without_an_error() {
		let mut progress = Vec::new();
		let change = DetailsChange {
			title: Some("New title".into()),
			..Default::default()
		};
		let result = apply_save(
			fields(&["2"]),
			&[PublishedFileId(3), PublishedFileId(4)],
			&[PublishedFileId(2)],
			change,
			|id, _| Ok(if id.0 == 4 { Paced::Stopped } else { Paced::Written }),
			|_| panic!("Details must not be saved after a stop"),
			|completed, total| progress.push((completed, total)),
		);
		assert!(result.stopped);
		assert!(result.error.is_none());
		assert_eq!(result.saved.items, vec!["2", "3"]);
		assert_eq!(result.saved.title, "Server pack");
		assert_eq!(progress, vec![(1, 3)]);
	}

	#[test]
	fn item_kinds_come_from_garrys_mod_type_tags() {
		let kind = |tags: &[&str]| ItemKind::of(&tags.iter().map(|tag| tag.to_string()).collect::<Vec<_>>());
		assert_eq!(kind(&["Addon", "Weapon", "Fun"]), ItemKind::Addon);
		assert_eq!(kind(&["Dupe", "Machines"]), ItemKind::Dupe);
		assert_eq!(kind(&["save", "Scenes"]), ItemKind::Save);
		assert_eq!(kind(&["demo"]), ItemKind::Demo);
		assert_eq!(kind(&["Fun"]), ItemKind::Other);
		assert_eq!(kind(&[]), ItemKind::Other);
	}

	fn change(collection: &str, add: &[&str], remove: &[&str]) -> CollectionChange {
		CollectionChange {
			collection_id: collection.into(),
			add: add.iter().map(|id| id.to_string()).collect(),
			remove: remove.iter().map(|id| id.to_string()).collect(),
		}
	}

	#[test]
	fn bulk_changes_reject_ambiguous_requests() {
		assert!(parse_changes(vec![change("1", &["2"], &[]), change("5", &[], &["3"])]).is_ok());
		for changes in [
			vec![],
			vec![change("1", &[], &[])],
			vec![change("1", &["2"], &[]), change("1", &["3"], &[])],
			vec![change("1", &["1"], &[])],
			vec![change("1", &["2"], &["2"])],
			vec![change("abc", &["2"], &[])],
		] {
			assert!(parse_changes(changes).is_err());
		}
	}

	#[test]
	fn bulk_changes_skip_members_and_stop_or_halt() {
		let parsed = parse_changes(vec![change("1", &["2", "3"], &["4", "5"])]).unwrap().pop().unwrap();
		let children = [PublishedFileId(2), PublishedFileId(4)];
		let mut writes = Vec::new();
		let mut steps = 0;
		let (outcome, flow) = apply_change(
			&parsed,
			&children,
			|item, add| {
				writes.push((item.0, add));
				Ok(Paced::Written)
			},
			|| steps += 1,
		);
		assert!(matches!(flow, Flow::Continue));
		assert_eq!(writes, vec![(3, true), (4, false)]);
		assert_eq!(outcome.changed, vec!["3", "4"]);
		assert_eq!(steps, 4);

		let (outcome, flow) = apply_change(&parsed, &children, |_, _| Ok(Paced::Stopped), || {});
		assert!(matches!(flow, Flow::Stopped));
		assert!(outcome.changed.is_empty() && outcome.error.is_none());

		let (outcome, flow) = apply_change(&parsed, &children, |_, _| Err(SteamWriteError::steam(SteamError::LimitExceeded)), || {});
		assert!(matches!(flow, Flow::Halted));
		assert!(outcome.error.unwrap().contains("refused more items"));

		let (_, flow) = apply_change(&parsed, &children, |_, _| Err(SteamWriteError::steam(SteamError::AccessDenied)), || {});
		assert!(matches!(flow, Flow::Continue));
	}
}
