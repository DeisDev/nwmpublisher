use std::{
	collections::{HashMap, HashSet},
	fs::{self, Metadata},
	path::{Path, PathBuf},
	sync::{
		atomic::{AtomicU64, Ordering},
		mpsc,
	},
	time::{Duration, Instant, SystemTime},
};

use parking_lot::Mutex;
use serde::Serialize;
use steamworks::{sys, AppIDs, ItemState, PublishedFileId, SteamId, UGCType, UserList, UserListOrder};
use tauri::ipc::Channel;

static SCAN: Mutex<Option<Snapshot>> = Mutex::new(None);
static NEXT_SCAN: AtomicU64 = AtomicU64::new(1);
const STEAM_TIMEOUT: Duration = Duration::from_secs(30);
const SUBSCRIPTION_CONCURRENCY: usize = 4;
const DETAILS_BATCH_SIZE: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Source {
	Addons,
	Cache,
	Workshop,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerProgress {
	stage: &'static str,
	completed: usize,
	total: Option<usize>,
	found: usize,
	source: Option<Source>,
	path: Option<PathBuf>,
}

struct Progress {
	channel: Option<Channel<CleanerProgress>>,
	state: CleanerProgress,
	last_sent: Instant,
}

impl Progress {
	fn new(channel: Channel<CleanerProgress>) -> Self {
		Self {
			channel: Some(channel),
			last_sent: Instant::now(),
			state: CleanerProgress {
				stage: "subscriptions",
				completed: 0,
				total: None,
				found: 0,
				source: None,
				path: None,
			},
		}
	}

	fn stage(&mut self, stage: &'static str, total: Option<usize>) {
		self.state = CleanerProgress {
			stage,
			completed: 0,
			total,
			found: 0,
			source: None,
			path: None,
		};
		self.send(true);
	}

	fn send(&mut self, force: bool) {
		if !force && self.last_sent.elapsed() < Duration::from_millis(100) {
			return;
		}
		self.last_sent = Instant::now();
		if let Some(channel) = &self.channel {
			if let Err(error) = channel.send(self.state.clone()) {
				eprintln!("Addon cleaner progress delivery failed: {error}");
				self.channel = None;
			}
		}
	}

	fn checking(&mut self, path: &Path, found: usize) {
		self.state.completed += 1;
		self.state.found = found;
		if self.last_sent.elapsed() >= Duration::from_millis(100) {
			self.state.path = Some(path.to_owned());
			self.send(false);
		}
	}
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerError {
	key: &'static str,
	detail: String,
}

impl CleanerError {
	pub(crate) fn new(key: &'static str) -> Self {
		Self { key, detail: String::new() }
	}
	pub(crate) fn detail(key: &'static str, detail: impl std::fmt::Display) -> Self {
		Self {
			key,
			detail: detail.to_string(),
		}
	}
	pub(crate) fn io(path: &Path, error: impl std::fmt::Display) -> Self {
		Self::detail("cleaner_error_file", format!("{}: {}", path.display(), error))
	}
}
impl std::fmt::Display for CleanerError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let text = crate::util::english::format(self.key, &[("detail", &self.detail)]);
		if self.detail.is_empty() || text.contains(&self.detail) {
			f.write_str(&text)
		} else {
			write!(f, "{} {}", text, self.detail)
		}
	}
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
	path: PathBuf,
	file_name: String,
	source: Source,
	workshop_id: String,
	size: u64,
	folder: bool,
	steam_cache: bool,
	#[serde(skip)]
	modified: SystemTime,
	#[serde(skip)]
	created: Option<SystemTime>,
}

impl Candidate {
	fn new(path: PathBuf, id: u64, source: Source, metadata: &Metadata) -> Result<Self, CleanerError> {
		Ok(Self {
			file_name: path.file_name().unwrap().to_string_lossy().into_owned(),
			source,
			folder: metadata.is_dir(),
			steam_cache: false,
			size: if metadata.is_dir() { 0 } else { metadata.len() },
			modified: metadata.modified().map_err(|error| CleanerError::io(&path, error))?,
			created: metadata.created().ok(),
			path,
			workshop_id: id.to_string(),
		})
	}
}

struct Snapshot {
	id: String,
	account: SteamId,
	gmod: PathBuf,
	files: Vec<Candidate>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
	scan_id: String,
	account: String,
	files: Vec<Candidate>,
}

#[derive(Serialize)]
pub struct FileFailure {
	pub(crate) path: PathBuf,
	pub(crate) error: CleanerError,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanResult {
	pub(crate) removed: Vec<PathBuf>,
	pub(crate) failed: Vec<FileFailure>,
	pub(crate) steam_cache: Vec<PathBuf>,
}

#[derive(Debug, PartialEq)]
enum Removal {
	Delete,
	Gone,
	SteamCache,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerDetails {
	id: String,
	title: Option<String>,
	preview_url: Option<String>,
}

fn account() -> Result<SteamId, CleanerError> {
	if !steam!().connected() || !steam!().client().user().logged_on() {
		return Err(CleanerError::new("cleaner_error_offline"));
	}
	Ok(steam!().client().user().steam_id())
}

/// Steam still counts deleted and private subscriptions in the total but leaves them out of the pages.
#[derive(Default)]
pub(crate) struct SubscriptionPages {
	pub(crate) ids: HashSet<PublishedFileId>,
	total: Option<u32>,
	missing: u32,
}

impl SubscriptionPages {
	pub(crate) fn append(&mut self, page: u32, total: u32, ids: Vec<PublishedFileId>) -> Result<(), CleanerError> {
		let page_size = steamworks::RESULTS_PER_PAGE;
		let expected = total.saturating_sub((page - 1) * page_size).min(page_size);
		if self.total.is_some_and(|previous| previous != total) || ids.len() > expected as usize {
			return Err(CleanerError::new("cleaner_error_subscriptions"));
		}
		self.total = Some(total);
		self.missing += expected - ids.len() as u32;
		for id in ids {
			if id.0 == 0 || !self.ids.insert(id) {
				return Err(CleanerError::new("cleaner_error_subscriptions"));
			}
		}
		Ok(())
	}

	pub(crate) fn received(&self) -> usize {
		self.ids.len() + self.missing as usize
	}

	pub(crate) fn complete(&self) -> bool {
		self.total == Some(self.received() as u32)
	}

	pub(crate) fn missing(&self) -> usize {
		self.missing as usize
	}
}

/// Trusts pages with items left out only when as many subscriptions known only to the Steam client are unavailable.
pub(crate) fn missing_explained(missing: usize, unavailable: usize) -> bool {
	if unavailable < missing {
		eprintln!("Steam left out {missing} subscriptions, but only {unavailable} subscriptions known only to the Steam client are unavailable");
		return false;
	}
	if missing > 0 {
		println!("Steam left out {missing} unavailable subscriptions");
	}
	true
}

/// Counts the items Steam can no longer show, such as deleted or private ones.
fn unavailable(ids: &[PublishedFileId]) -> Result<usize, CleanerError> {
	let mut count = 0;
	for chunk in ids.chunks(DETAILS_BATCH_SIZE) {
		let requested: HashSet<PublishedFileId> = chunk.iter().copied().collect();
		let (sender, receiver) = mpsc::sync_channel(1);
		steam!()
			.client()
			.ugc()
			.query_items(chunk.to_vec())
			.map_err(|error| CleanerError::detail("cleaner_error_subscriptions", error))?
			.allow_cached_response(0)
			.fetch(move |response| {
				let found = response
					.map(|results| {
						results
							.iter()
							.flatten()
							.map(|item| item.published_file_id)
							.filter(|id| requested.contains(id))
							.collect::<HashSet<_>>()
							.len()
					})
					.map_err(|error| CleanerError::detail("cleaner_error_subscriptions", error));
				let _ = sender.send(found);
			});
		let found = receiver
			.recv_timeout(STEAM_TIMEOUT)
			.map_err(|error| CleanerError::detail("cleaner_error_subscriptions", error))??;
		count += chunk.len() - found;
	}
	Ok(count)
}

type SubscriptionPage = Result<(u32, Vec<PublishedFileId>), CleanerError>;

fn request_subscription_page(owner: SteamId, page: u32, sender: mpsc::Sender<(u32, SubscriptionPage)>) -> Result<(), CleanerError> {
	if account()? != owner {
		return Err(CleanerError::new("cleaner_error_account"));
	}
	steam!()
		.client()
		.ugc()
		.query_user(
			owner.account_id(),
			UserList::Subscribed,
			UGCType::All,
			UserListOrder::SubscriptionDateDesc,
			AppIDs::ConsumerAppId(crate::GMOD_APP_ID),
			page,
		)
		.map_err(|error| CleanerError::detail("cleaner_error_subscriptions", error))?
		.allow_cached_response(0)
		.set_return_only_ids(true)
		.fetch(move |result| {
			let result = result
				.map_err(|error| CleanerError::detail("cleaner_error_subscriptions", error))
				.map(|results| {
					let ids = (0..results.returned_results())
						.filter_map(|index| results.get(index).map(|item| item.published_file_id))
						.collect();
					(results.total_results(), ids)
				});
			let _ = sender.send((page, result));
		});
	Ok(())
}

fn subscriptions(owner: SteamId, progress: &mut Progress) -> Result<HashSet<PublishedFileId>, CleanerError> {
	progress.stage("subscriptions", None);
	let mut pages = SubscriptionPages::default();
	let (sender, receiver) = mpsc::channel();
	let mut pending = HashMap::from([(1, Instant::now())]);
	request_subscription_page(owner, 1, sender.clone())?;
	let mut next_page = 2;
	while !pending.is_empty() {
		let remaining = pending
			.values()
			.map(|started| STEAM_TIMEOUT.saturating_sub(started.elapsed()))
			.min()
			.unwrap();
		let (page, response) = receiver
			.recv_timeout(remaining)
			.map_err(|error| CleanerError::detail("cleaner_error_subscriptions", error))?;
		if pending.remove(&page).is_none() {
			return Err(CleanerError::new("cleaner_error_subscriptions"));
		}
		let (total, page_ids) = response?;
		pages.append(page, total, page_ids)?;
		progress.state.completed = pages.received();
		progress.state.total = Some(total as usize);
		progress.send(true);
		let page_count = total.div_ceil(steamworks::RESULTS_PER_PAGE);
		while pending.len() < SUBSCRIPTION_CONCURRENCY && next_page <= page_count {
			pending.insert(next_page, Instant::now());
			request_subscription_page(owner, next_page, sender.clone())?;
			next_page += 1;
		}
	}
	if !pages.complete() {
		return Err(CleanerError::new("cleaner_error_subscriptions"));
	}
	if account()? != owner {
		return Err(CleanerError::new("cleaner_error_account"));
	}
	let local: HashSet<PublishedFileId> = steam!().client().ugc().subscribed_items(true).into_iter().collect();
	if pages.missing() > 0 {
		let (unsubscribed, _) = game_addons!().session_subscriptions();
		let unlisted: Vec<_> = local.iter().filter(|id| !pages.ids.contains(id) && !unsubscribed.contains(id)).copied().collect();
		if !missing_explained(pages.missing(), unavailable(&unlisted)?) {
			return Err(CleanerError::new("cleaner_error_subscriptions"));
		}
	}
	// Keep local subscriptions too, including items missing from a server query and items disabled locally in Steam.
	let mut ids = pages.ids;
	ids.extend(local);
	Ok(ids)
}

fn downloaded_items() -> Result<HashSet<PublishedFileId>, CleanerError> {
	let _client = steam!().client();
	unsafe {
		let ugc = sys::SteamAPI_SteamUGC_v021();
		if ugc.is_null() {
			return Err(CleanerError::new("cleaner_error_offline"));
		}
		let mut ids = vec![0; sys::SteamAPI_ISteamUGC_GetNumDownloadedItems(ugc) as usize];
		let count = sys::SteamAPI_ISteamUGC_GetDownloadedItems(ugc, ids.as_mut_ptr(), ids.len() as u32);
		ids.truncate(count as usize);
		Ok(ids.into_iter().map(PublishedFileId).collect())
	}
}

fn mark_unused(id: PublishedFileId) -> bool {
	let _client = steam!().client();
	unsafe {
		let ugc = sys::SteamAPI_SteamUGC_v021();
		!ugc.is_null() && sys::SteamAPI_ISteamUGC_MarkDownloadedItemAsUnused(ugc, id.0)
	}
}

/// Steam restores the files of items it still needs, so it has to let go of the item first.
/// It refuses items it no longer needs, which it leaves on disk without restoring them.
pub(crate) fn release_item(id: PublishedFileId, state: ItemState) -> bool {
	let released = mark_unused(id);
	if !released {
		wprintln!("Steam would not mark Workshop item {} as unused (state {:?}); deleting its files", id.0, state);
	}
	released
}

pub(crate) fn is_link(metadata: &Metadata) -> bool {
	#[cfg(windows)]
	{
		use std::os::windows::fs::MetadataExt;
		metadata.file_attributes() & 0x400 != 0
	}
	#[cfg(not(windows))]
	{
		metadata.file_type().is_symlink()
	}
}

fn check_ancestors(path: &Path) -> Result<(), CleanerError> {
	if !path.is_absolute() || path.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
		return Err(CleanerError::new("cleaner_error_changed"));
	}
	for ancestor in path.ancestors() {
		let metadata = fs::symlink_metadata(ancestor).map_err(|error| CleanerError::io(ancestor, error))?;
		if is_link(&metadata) {
			return Err(CleanerError::new("cleaner_error_link"));
		}
	}
	Ok(())
}

fn numeric_id(value: &str) -> Option<u64> {
	if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
		return None;
	}
	value.parse().ok().filter(|id| *id > 0)
}

fn archive_id(path: &Path, legacy: bool) -> Option<u64> {
	if !path.extension()?.eq_ignore_ascii_case("gma") {
		return None;
	}
	let stem = path.file_stem()?.to_str()?;
	let stem = stem.strip_prefix("ds_").unwrap_or(stem);
	numeric_id(stem).or_else(|| if legacy { numeric_id(stem.rsplit_once('_')?.1) } else { None })
}

fn is_workshop_archive(path: &Path) -> bool {
	path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("gma"))
		|| path
			.file_name()
			.and_then(|name| name.to_str())
			.and_then(|name| name.strip_suffix("_legacy.bin"))
			.and_then(numeric_id)
			.is_some()
}

fn entries(path: &Path) -> Result<Vec<PathBuf>, CleanerError> {
	match fs::symlink_metadata(path) {
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
		Err(error) => return Err(CleanerError::io(path, error)),
		Ok(_) => check_ancestors(path)?,
	}
	read_entries(path)
}

fn read_entries(path: &Path) -> Result<Vec<PathBuf>, CleanerError> {
	fs::read_dir(path)
		.map_err(|error| CleanerError::io(path, error))?
		.map(|entry| entry.map(|entry| entry.path()).map_err(|error| CleanerError::io(path, error)))
		.collect()
}

fn candidate(path: PathBuf, id: u64, source: Source, subscribed: &HashSet<PublishedFileId>) -> Result<Option<Candidate>, CleanerError> {
	if subscribed.contains(&PublishedFileId(id)) {
		return Ok(None);
	}
	let metadata = fs::symlink_metadata(&path).map_err(|error| CleanerError::io(&path, error))?;
	if !metadata.is_file() || is_link(&metadata) {
		return Ok(None);
	}
	Candidate::new(path, id, source, &metadata).map(Some)
}

fn discover(
	gmod: &Path,
	subscribed: &HashSet<PublishedFileId>,
	steam_cache: &HashSet<PublishedFileId>,
	progress: &mut Progress,
) -> Result<Vec<Candidate>, CleanerError> {
	check_ancestors(gmod)?;
	progress.stage("files", None);
	let mut files = Vec::new();
	for (folder, source) in [
		(gmod.join("garrysmod/addons"), Source::Addons),
		(gmod.join("garrysmod/cache/workshop"), Source::Cache),
	] {
		progress.state.source = Some(source);
		progress.state.path = Some(folder.clone());
		progress.send(true);
		for path in entries(&folder)? {
			if let Some(id) = archive_id(&path, source == Source::Addons) {
				if let Some(file) = candidate(path.clone(), id, source, subscribed)? {
					files.push(file);
				}
			}
			progress.checking(&path, files.len());
		}
	}
	if let Some(steamapps) = gmod.parent().and_then(Path::parent) {
		let content = steamapps.join("workshop/content/4000");
		progress.state.source = Some(Source::Workshop);
		progress.state.path = Some(content.clone());
		progress.send(true);
		for folder in entries(&content)? {
			progress.checking(&folder, files.len());
			let Some(id) = folder.file_name().and_then(|name| name.to_str()).and_then(numeric_id) else {
				continue;
			};
			if subscribed.contains(&PublishedFileId(id)) {
				continue;
			}
			let metadata = fs::symlink_metadata(&folder).map_err(|error| CleanerError::io(&folder, error))?;
			if !metadata.is_dir() || is_link(&metadata) {
				continue;
			}
			let first = files.len();
			// The content root was checked above and this child is a regular directory.
			let paths = read_entries(&folder)?;
			if paths.is_empty() {
				files.push(Candidate::new(folder, id, Source::Workshop, &metadata)?);
			}
			for path in paths {
				if is_workshop_archive(&path) {
					if let Some(file) = candidate(path.clone(), id, Source::Workshop, subscribed)? {
						files.push(file);
					}
				}
				progress.checking(&path, files.len());
			}
			let cached = steam_cache.contains(&PublishedFileId(id));
			files[first..].iter_mut().for_each(|file| file.steam_cache = cached);
		}
	}
	progress.state.found = files.len();
	progress.send(true);
	files.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
	Ok(files)
}

fn unchanged(file: &Candidate) -> Result<(), CleanerError> {
	check_ancestors(&file.path)?;
	let metadata = fs::symlink_metadata(&file.path).map_err(|error| CleanerError::io(&file.path, error))?;
	let same = if file.folder {
		metadata.is_dir() && read_entries(&file.path)?.is_empty()
	} else {
		metadata.is_file()
			&& metadata.len() == file.size
			&& metadata.modified().map_err(|error| CleanerError::io(&file.path, error))? == file.modified
			&& metadata.created().ok() == file.created
	};
	if !same {
		return Err(CleanerError::new("cleaner_error_changed"));
	}
	Ok(())
}

pub(crate) fn remove_empty_folder(folder: &Path) -> Result<(), CleanerError> {
	if read_entries(folder)?.is_empty() {
		fs::remove_dir(folder).map_err(|error| CleanerError::io(folder, error))?;
	}
	Ok(())
}

pub(crate) fn remove_file(path: &Path, permanent: bool) -> Result<(), CleanerError> {
	if permanent {
		return fs::remove_file(path).map_err(|error| CleanerError::io(path, error));
	}
	trash::delete(path).map_err(|error| CleanerError::io(path, error))?;
	// Some shell operations can report success even when the user cancels.
	match fs::symlink_metadata(path) {
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
		Err(error) => Err(CleanerError::io(path, error)),
		Ok(_) => Err(CleanerError::new("cleaner_error_retained")),
	}
}

/// Garry's Mod's copies of one Workshop item. Steam removes its own copy once the game closes, and still reports
/// the item as subscribed right after unsubscribing, so its content folder is left alone.
pub(crate) fn item_archives(gmod: &Path, id: PublishedFileId, selected: &Path) -> Result<Vec<PathBuf>, CleanerError> {
	let mut files = item_archives_by_id(gmod, &HashSet::from([id]))?.remove(&id).unwrap_or_default();
	if crate::game_addons::GameAddons::workshop_content_id(selected).is_none() {
		files.push(selected.to_owned());
	}
	Ok(regular_files(files))
}

/// Like `item_archives`, for many items that may never have been installed, reading each folder once.
pub(crate) fn item_archives_by_id(gmod: &Path, ids: &HashSet<PublishedFileId>) -> Result<HashMap<PublishedFileId, Vec<PathBuf>>, CleanerError> {
	check_ancestors(gmod)?;
	let mut found: HashMap<PublishedFileId, Vec<PathBuf>> = HashMap::new();
	for (folder, legacy) in [(gmod.join("garrysmod/addons"), true), (gmod.join("garrysmod/cache/workshop"), false)] {
		for path in entries(&folder)? {
			if let Some(id) = archive_id(&path, legacy).map(PublishedFileId).filter(|id| ids.contains(id)) {
				found.entry(id).or_default().push(path);
			}
		}
	}
	Ok(found.into_iter().map(|(id, files)| (id, regular_files(files))).filter(|(_, files)| !files.is_empty()).collect())
}

fn regular_files(mut files: Vec<PathBuf>) -> Vec<PathBuf> {
	let mut seen = HashSet::new();
	files.retain(|path| {
		fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file() && !is_link(&metadata))
			&& seen.insert(dunce::canonicalize(path).unwrap_or_else(|_| path.clone()))
	});
	files
}

pub(crate) fn remove_archives(files: Vec<PathBuf>, permanent: bool) -> CleanResult {
	let mut result = CleanResult::default();
	for path in files {
		match remove_file(&path, permanent) {
			Ok(()) => result.removed.push(path),
			Err(error) => result.failed.push(FileFailure { path, error }),
		}
	}
	result
}

fn selected_files(snapshot: &Snapshot, id: &str, selected: &[usize]) -> Result<Vec<Candidate>, CleanerError> {
	if snapshot.id != id || selected.is_empty() || selected.iter().collect::<HashSet<_>>().len() != selected.len() {
		return Err(CleanerError::new("cleaner_error_scan"));
	}
	selected
		.iter()
		.map(|index| snapshot.files.get(*index).cloned().ok_or_else(|| CleanerError::new("cleaner_error_scan")))
		.collect()
}

fn detail_ids(snapshot: &Snapshot, scan_id: &str, selected: &[usize]) -> Result<Vec<PublishedFileId>, CleanerError> {
	if selected.len() > DETAILS_BATCH_SIZE {
		return Err(CleanerError::new("cleaner_error_scan"));
	}
	let mut seen = HashSet::new();
	Ok(selected_files(snapshot, scan_id, selected)?
		.iter()
		.map(|file| PublishedFileId(file.workshop_id.parse().unwrap()))
		.filter(|id| seen.insert(*id))
		.collect())
}

#[tauri::command]
pub async fn addon_cleaner_details(scan_id: String, selected: Vec<usize>) -> Result<Vec<CleanerDetails>, CleanerError> {
	tauri::async_runtime::spawn_blocking(move || {
		let owner = account()?;
		let ids = {
			let scan = SCAN.try_lock().ok_or_else(|| CleanerError::new("cleaner_error_busy"))?;
			let snapshot = scan.as_ref().ok_or_else(|| CleanerError::new("cleaner_error_scan"))?;
			if owner != snapshot.account {
				return Err(CleanerError::new("cleaner_error_account"));
			}
			detail_ids(snapshot, &scan_id, &selected)?
		};
		let (sender, receiver) = mpsc::sync_channel(1);
		steam!()
			.client()
			.ugc()
			.query_items(ids.clone())
			.map_err(|error| CleanerError::detail("cleaner_error_details", error))?
			.allow_cached_response(600)
			.fetch(move |response| {
				let response = response
					.map_err(|error| CleanerError::detail("cleaner_error_details", error))
					.and_then(|results| {
						ids.iter()
							.enumerate()
							.map(|(index, id)| {
								let item = results.get(index as u32);
								if item
									.as_ref()
									.is_some_and(|item| item.published_file_id != *id || item.consumer_app_id != Some(crate::GMOD_APP_ID))
								{
									return Err(CleanerError::new("cleaner_error_details"));
								}
								Ok(CleanerDetails {
									id: id.0.to_string(),
									preview_url: item.as_ref().and_then(|_| results.preview_url(index as u32)),
									title: item.map(|item| item.title),
								})
							})
							.collect()
					});
				let _ = sender.send(response);
			});
		let details = receiver
			.recv_timeout(STEAM_TIMEOUT)
			.map_err(|error| CleanerError::detail("cleaner_error_details", error))??;
		if account()? != owner {
			return Err(CleanerError::new("cleaner_error_account"));
		}
		Ok(details)
	})
	.await
	.map_err(|error| CleanerError::detail("cleaner_error_task", error))?
}

fn remove_files(
	files: Vec<Candidate>,
	permanent: bool,
	progress: &mut Progress,
	mut authorize: impl FnMut(&Candidate) -> Result<Removal, CleanerError>,
) -> CleanResult {
	progress.stage(if permanent { "deleting" } else { "recycling" }, Some(files.len()));
	let mut result = CleanResult::default();
	for file in files {
		progress.state.path = Some(file.path.clone());
		progress.send(progress.state.completed == 0);
		let removal = authorize(&file).and_then(|removal| {
			if removal == Removal::Delete {
				unchanged(&file)?;
				if file.folder {
					fs::remove_dir(&file.path).map_err(|error| CleanerError::io(&file.path, error))?;
				} else {
					remove_file(&file.path, permanent)?;
				}
			}
			Ok(removal)
		});
		match removal {
			Ok(Removal::SteamCache) => result.steam_cache.push(file.path),
			Ok(removal) => {
				if removal == Removal::Delete && file.source == Source::Workshop && !file.folder {
					let folder = file.path.parent().unwrap();
					if let Err(error) = remove_empty_folder(folder) {
						result.failed.push(FileFailure {
							path: folder.to_owned(),
							error,
						});
					}
				}
				result.removed.push(file.path);
			}
			Err(error) => result.failed.push(FileFailure { path: file.path, error }),
		}
		progress.state.completed += 1;
		progress.state.found = result.removed.len();
		progress.send(progress.state.total == Some(progress.state.completed));
	}
	result
}

#[tauri::command]
pub async fn scan_addon_cleaner(on_progress: Channel<CleanerProgress>) -> Result<ScanResult, CleanerError> {
	let result = tauri::async_runtime::spawn_blocking(move || {
		let mut progress = Progress::new(on_progress);
		let mut scan = SCAN.try_lock().ok_or_else(|| CleanerError::new("cleaner_error_busy"))?;
		*scan = None;
		let owner = account()?;
		let subscribed = subscriptions(owner, &mut progress)?;
		progress.stage("locating", None);
		let gmod = app_data!().gmod_dir().ok_or_else(|| CleanerError::new("cleaner_error_gmod"))?;
		let files = discover(&gmod, &subscribed, &downloaded_items()?, &mut progress)?;
		let id = NEXT_SCAN.fetch_add(1, Ordering::Relaxed).to_string();
		let result = ScanResult {
			scan_id: id.clone(),
			account: owner.raw().to_string(),
			files: files.clone(),
		};
		*scan = Some(Snapshot {
			id,
			account: owner,
			gmod,
			files,
		});
		Ok(result)
	})
	.await
	.map_err(|error| CleanerError::detail("cleaner_error_task", error))
	.and_then(std::convert::identity);
	match &result {
		Ok(scan) => println!(
			"Addon cleaner scan found {} removable files, {} in Steam's cache",
			scan.files.len(),
			scan.files.iter().filter(|file| file.steam_cache).count()
		),
		Err(error) => eprintln!("Addon cleaner scan failed: {}", error),
	}
	result
}

#[tauri::command]
pub async fn clean_addons(
	scan_id: String,
	selected: Vec<usize>,
	permanent: bool,
	on_progress: Channel<CleanerProgress>,
) -> Result<CleanResult, CleanerError> {
	let result = tauri::async_runtime::spawn_blocking(move || {
		let mut progress = Progress::new(on_progress);
		let mut scan = SCAN.try_lock().ok_or_else(|| CleanerError::new("cleaner_error_busy"))?;
		let snapshot = scan.as_ref().ok_or_else(|| CleanerError::new("cleaner_error_scan"))?;
		let files = selected_files(snapshot, &scan_id, &selected)?;
		let owner = account()?;
		if owner != snapshot.account {
			return Err(CleanerError::new("cleaner_error_account"));
		}
		if app_data!().gmod_dir().as_ref() != Some(&snapshot.gmod) {
			return Err(CleanerError::new("cleaner_error_changed"));
		}
		let subscribed = subscriptions(owner, &mut progress)?;
		let mut marked = HashMap::new();
		let result = remove_files(files, permanent, &mut progress, |file| {
			if account()? != owner {
				return Err(CleanerError::new("cleaner_error_account"));
			}
			let id = PublishedFileId(file.workshop_id.parse().unwrap());
			let state = steam!().client().ugc().item_state(id);
			if subscribed.contains(&id) || state.intersects(ItemState::SUBSCRIBED | ItemState::DOWNLOADING | ItemState::DOWNLOAD_PENDING) {
				return Err(CleanerError::new("cleaner_error_subscribed"));
			}
			if file.source != Source::Workshop || state.is_empty() {
				return Ok(Removal::Delete);
			}
			unchanged(file)?;
			if !*marked.entry(id).or_insert_with(|| release_item(id, state)) {
				return Ok(Removal::Delete);
			}
			if fs::symlink_metadata(&file.path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
				return Ok(Removal::Gone);
			}
			if !steam!().client().ugc().item_state(id).is_empty() {
				return Ok(Removal::SteamCache);
			}
			Ok(Removal::Delete)
		});
		*scan = None;
		if !result.removed.is_empty() {
			crate::commands::free_caches();
			webview_emit!("InstalledAddonsRefreshed");
		}
		Ok(result)
	})
	.await
	.map_err(|error| CleanerError::detail("cleaner_error_task", error))
	.and_then(std::convert::identity);
	match &result {
		Ok(clean) => {
			println!(
				"Addon cleaner {} {} files and left {} in Steam's cache",
				if permanent { "deleted" } else { "moved to Trash" },
				clean.removed.len(),
				clean.steam_cache.len()
			);
			for failure in &clean.failed {
				wprintln!("Addon cleaner kept {}: {}", failure.path.display(), failure.error);
			}
		}
		Err(error) => eprintln!("Addon cleanup failed: {}", error),
	}
	result
}

#[cfg(test)]
mod tests {
	use super::*;

	fn quiet_progress() -> Progress {
		Progress::new(Channel::new(|_| Ok(())))
	}

	#[test]
	fn only_recognizes_workshop_archive_names() {
		for (name, expected) in [
			("123.gma", Some(123)),
			("ds_123.gma", Some(123)),
			("some_addon_123.GMA", Some(123)),
			("myaddon2.gma", None),
			("0.gma", None),
			("123.zip", None),
			("18446744073709551616.gma", None),
		] {
			assert_eq!(archive_id(Path::new(name), true), expected, "{name}");
		}
		assert_eq!(archive_id(Path::new("addon_123.gma"), false), None);
		for (name, expected) in [
			("content.GMA", true),
			("1099143090259692554_legacy.bin", true),
			("_legacy.bin", false),
			("data.bin", false),
			("notes.txt", false),
		] {
			assert_eq!(is_workshop_archive(Path::new(name)), expected, "{name}");
		}
	}

	#[test]
	fn changing_or_inconsistent_subscription_pages_fail_closed() {
		let ids = |range: std::ops::RangeInclusive<u64>| range.map(PublishedFileId).collect::<Vec<_>>();
		let mut pages = SubscriptionPages::default();
		pages.append(1, 51, ids(1..=50)).unwrap();
		assert!(pages.append(2, 52, ids(51..=51)).is_err());
		assert!(SubscriptionPages::default().append(1, 2, ids(1..=3)).is_err());
		assert!(SubscriptionPages::default().append(1, 2, vec![PublishedFileId(1), PublishedFileId(1)]).is_err());
		assert!(SubscriptionPages::default().append(1, 1, vec![PublishedFileId(0)]).is_err());
		let mut pages = SubscriptionPages::default();
		pages.append(1, 0, Vec::new()).unwrap();
		assert!(pages.complete());
	}

	#[test]
	fn subscription_pages_count_items_steam_leaves_out() {
		let mut pages = SubscriptionPages::default();
		pages.append(2, 100, (51..=99).map(PublishedFileId).collect()).unwrap();
		assert!(!pages.complete());
		pages.append(1, 100, Vec::new()).unwrap();
		assert!(pages.complete());
		assert_eq!(pages.missing(), 51);
		assert_eq!(pages.received(), 100);
		assert!(!missing_explained(51, 50));
		assert!(missing_explained(51, 51));
		assert!(missing_explained(0, 0));
	}

	#[test]
	fn subscription_pages_can_complete_out_of_order_without_losing_ids() {
		let mut pages = SubscriptionPages::default();
		for page in [1, 4, 2, 3] {
			let start = (page - 1) * 50 + 1;
			pages.append(page as u32, 200, (start..start + 50).map(PublishedFileId).collect()).unwrap();
			assert_eq!(pages.complete(), page == 3);
		}
		assert_eq!(pages.ids, (1..=200).map(PublishedFileId).collect());
		assert_eq!(pages.missing(), 0);
	}

	#[test]
	fn workshop_details_are_bounded_and_duplicate_copies_share_one_lookup() {
		let file = Candidate {
			path: PathBuf::from("123.gma"),
			file_name: "123.gma".into(),
			source: Source::Cache,
			workshop_id: "123".into(),
			size: 1,
			folder: false,
			steam_cache: false,
			modified: SystemTime::UNIX_EPOCH,
			created: None,
		};
		let mut files = vec![file; 1000];
		files[2].workshop_id = "456".into();
		let snapshot = Snapshot {
			id: "scan".into(),
			account: SteamId::from_raw(1),
			gmod: PathBuf::new(),
			files,
		};
		assert_eq!(
			detail_ids(&snapshot, "scan", &[0, 1, 2]).unwrap(),
			vec![PublishedFileId(123), PublishedFileId(456)]
		);
		assert!(detail_ids(&snapshot, "scan", &(0..50).collect::<Vec<_>>()).is_ok());
		assert!(detail_ids(&snapshot, "scan", &(0..51).collect::<Vec<_>>()).is_err());
		assert!(detail_ids(&snapshot, "old", &[0]).is_err());
		assert!(detail_ids(&snapshot, "scan", &[1000]).is_err());
	}

	#[test]
	fn scans_all_locations_and_preserves_subscriptions_and_local_content() {
		let root = tempfile::tempdir().unwrap();
		let root_path = fs::canonicalize(root.path()).unwrap();
		let gmod = root_path.join("steamapps/common/GarrysMod");
		for path in [
			"steamapps/common/GarrysMod/garrysmod/addons/name_10.gma",
			"steamapps/common/GarrysMod/garrysmod/addons/local.gma",
			"steamapps/common/GarrysMod/garrysmod/addons/myaddon2.gma",
			"steamapps/common/GarrysMod/garrysmod/cache/workshop/20.gma",
			"steamapps/workshop/content/4000/30/content.gma",
			"steamapps/workshop/content/4000/40/content.gma",
			"steamapps/workshop/content/4000/50/content.gma",
			"steamapps/workshop/content/4000/70/123_legacy.bin",
			"steamapps/workshop/content/4000/30/keep.txt",
			"steamapps/workshop/content/4000/30/data.bin",
			"steamapps/workshop/content/4000/30/source/nested.gma",
		] {
			let path = root_path.join(path);
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(path, b"test").unwrap();
		}
		for folder in ["40/empty", "60", "80"] {
			fs::create_dir_all(root_path.join("steamapps/workshop/content/4000").join(folder)).unwrap();
		}
		let events = std::sync::Arc::new(Mutex::new(Vec::new()));
		let received = events.clone();
		let mut progress = Progress::new(Channel::new(move |body| {
			if let tauri::ipc::InvokeResponseBody::Json(json) = body {
				received.lock().push(serde_json::from_str::<serde_json::Value>(&json).unwrap());
			}
			Ok(())
		}));
		let steam_cache = HashSet::from([PublishedFileId(20), PublishedFileId(50), PublishedFileId(80)]);
		let files = discover(&gmod, &HashSet::from([PublishedFileId(40)]), &steam_cache, &mut progress).unwrap();
		assert_eq!(files.len(), 7);
		assert_eq!(files.iter().find(|file| file.workshop_id == "30").unwrap().source, Source::Workshop);
		let empty = files.iter().find(|file| file.workshop_id == "60").unwrap();
		assert!(empty.folder);
		assert_eq!(empty.size, 0);
		assert!(files
			.iter()
			.filter(|file| !["60", "80"].contains(&file.workshop_id.as_str()))
			.all(|file| !file.folder));
		assert_eq!(
			files
				.iter()
				.filter(|file| file.steam_cache)
				.map(|file| file.workshop_id.as_str())
				.collect::<HashSet<_>>(),
			HashSet::from(["50", "80"])
		);
		let events = events.lock();
		assert_eq!(events.last().unwrap()["found"], 7);
		assert!(events.last().unwrap()["completed"].as_u64().unwrap() > 5);
		for source in ["addons", "cache", "workshop"] {
			assert!(events.iter().any(|event| event["source"] == source));
		}
		assert_eq!(
			files.iter().map(|file| file.workshop_id.as_str()).collect::<HashSet<_>>(),
			HashSet::from(["10", "20", "30", "50", "60", "70", "80"])
		);
	}

	#[test]
	fn unsubscribing_removes_garrys_mod_copies_and_leaves_steams_copy() {
		let root = tempfile::tempdir().unwrap();
		let root_path = fs::canonicalize(root.path()).unwrap();
		let gmod = root_path.join("steamapps/common/GarrysMod");
		for path in [
			"steamapps/common/GarrysMod/garrysmod/addons/name_10.gma",
			"steamapps/common/GarrysMod/garrysmod/addons/named10.gma",
			"steamapps/common/GarrysMod/garrysmod/addons/name_100.gma",
			"steamapps/common/GarrysMod/garrysmod/cache/workshop/10.gma",
			"steamapps/common/GarrysMod/garrysmod/cache/workshop/ds_10.gma",
			"steamapps/common/GarrysMod/garrysmod/cache/workshop/20.gma",
			"steamapps/workshop/content/4000/10/content.gma",
		] {
			let path = root_path.join(path);
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(path, b"test").unwrap();
		}
		let selected = gmod.join("garrysmod/addons/named10.gma");
		let mut found = item_archives(&gmod, PublishedFileId(10), &selected).unwrap();
		found.sort();
		let mut expected: Vec<_> = [
			"steamapps/common/GarrysMod/garrysmod/addons/name_10.gma",
			"steamapps/common/GarrysMod/garrysmod/addons/named10.gma",
			"steamapps/common/GarrysMod/garrysmod/cache/workshop/10.gma",
			"steamapps/common/GarrysMod/garrysmod/cache/workshop/ds_10.gma",
		]
		.iter()
		.map(|path| root_path.join(path))
		.collect();
		expected.sort();
		assert_eq!(found, expected);
		assert_eq!(item_archives(&gmod, PublishedFileId(10), &gmod.join("garrysmod/addons/name_10.gma")).unwrap().len(), 3);
		let steam_copy = root_path.join("steamapps/workshop/content/4000/10/content.gma");
		assert_eq!(item_archives(&gmod, PublishedFileId(10), &steam_copy).unwrap().len(), 3);
		let by_id = item_archives_by_id(&gmod, &HashSet::from([PublishedFileId(10), PublishedFileId(20), PublishedFileId(30)])).unwrap();
		assert_eq!(by_id[&PublishedFileId(10)].len(), 3);
		assert_eq!(by_id[&PublishedFileId(20)], vec![gmod.join("garrysmod/cache/workshop/20.gma")]);
		assert!(!by_id.contains_key(&PublishedFileId(30)));

		let result = remove_archives(found, true);
		assert_eq!(result.removed.len(), 4);
		assert!(result.failed.is_empty());
		assert!(steam_copy.exists());
		assert!(gmod.join("garrysmod/addons/name_100.gma").exists());
		assert!(gmod.join("garrysmod/cache/workshop/20.gma").exists());
	}

	#[test]
	fn removing_workshop_content_clears_empty_item_folders_only() {
		let root = tempfile::tempdir().unwrap();
		let root_path = fs::canonicalize(root.path()).unwrap();
		for path in ["1/content.gma", "2/content.gma", "2/keep.txt"] {
			let path = root_path.join(path);
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(path, b"test").unwrap();
		}
		let mut files = Vec::new();
		for id in 1..=2 {
			files.push(
				candidate(root_path.join(format!("{id}/content.gma")), id, Source::Workshop, &HashSet::new())
					.unwrap()
					.unwrap(),
			);
		}
		for id in 3..=4 {
			let folder = root_path.join(id.to_string());
			fs::create_dir(&folder).unwrap();
			let metadata = fs::symlink_metadata(&folder).unwrap();
			files.push(Candidate::new(folder, id, Source::Workshop, &metadata).unwrap());
		}
		fs::write(root_path.join("4/new.gma"), b"test").unwrap();
		let result = remove_files(files, true, &mut quiet_progress(), |_| Ok(Removal::Delete));
		assert_eq!(
			result.removed,
			vec![root_path.join("1/content.gma"), root_path.join("2/content.gma"), root_path.join("3")]
		);
		assert_eq!(result.failed.len(), 1);
		assert_eq!(result.failed[0].path, root_path.join("4"));
		assert!(!root_path.join("1").exists());
		assert!(root_path.join("2/keep.txt").exists());
		assert!(!root_path.join("3").exists());
		assert!(root_path.join("4/new.gma").exists());
	}

	#[test]
	fn deletion_revalidates_selection_files_and_authorization() {
		let root = tempfile::tempdir().unwrap();
		let root_path = fs::canonicalize(root.path()).unwrap();
		let mut files = Vec::new();
		for id in 1..=4 {
			let path = root_path.join(format!("{id}.gma"));
			fs::write(&path, b"original").unwrap();
			files.push(candidate(path, id, Source::Cache, &HashSet::new()).unwrap().unwrap());
		}
		let snapshot = Snapshot {
			id: "scan".into(),
			account: SteamId::from_raw(1),
			gmod: root_path,
			files,
		};
		assert!(selected_files(&snapshot, "old", &[0]).is_err());
		assert!(selected_files(&snapshot, "scan", &[4]).is_err());
		assert!(selected_files(&snapshot, "scan", &[0, 0]).is_err());
		fs::write(&snapshot.files[1].path, b"changed file").unwrap();
		let mut progress = quiet_progress();
		let result = remove_files(selected_files(&snapshot, "scan", &[0, 1, 2]).unwrap(), true, &mut progress, |file| {
			if file.workshop_id == "3" {
				Err(CleanerError::new("cleaner_error_subscribed"))
			} else {
				Ok(Removal::Delete)
			}
		});
		assert_eq!(result.removed, vec![snapshot.files[0].path.clone()]);
		assert_eq!(result.failed.len(), 2);
		assert_eq!(progress.state.completed, 3);
		assert_eq!(progress.state.total, Some(3));
		assert_eq!(progress.state.found, 1);
		assert!(!snapshot.files[0].path.exists());
		assert!(snapshot.files[1..].iter().all(|file| file.path.exists()));
	}

	#[test]
	fn files_left_in_or_removed_by_steams_cache_are_reported_without_deleting() {
		let root = tempfile::tempdir().unwrap();
		let root_path = fs::canonicalize(root.path()).unwrap();
		let mut files = Vec::new();
		for id in 1..=3 {
			let path = root_path.join(format!("{id}/content.gma"));
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(&path, b"test").unwrap();
			files.push(candidate(path, id, Source::Workshop, &HashSet::new()).unwrap().unwrap());
		}
		let mut progress = quiet_progress();
		let result = remove_files(files, true, &mut progress, |file| match file.workshop_id.as_str() {
			"1" => Ok(Removal::SteamCache),
			"2" => {
				fs::remove_file(&file.path).unwrap();
				Ok(Removal::Gone)
			}
			_ => Ok(Removal::Delete),
		});
		assert_eq!(result.steam_cache, vec![root_path.join("1/content.gma")]);
		assert_eq!(result.removed, vec![root_path.join("2/content.gma"), root_path.join("3/content.gma")]);
		assert!(result.failed.is_empty());
		assert_eq!(progress.state.found, 2);
		assert!(root_path.join("1/content.gma").exists());
		assert!(!root_path.join("3").exists());
	}

	#[cfg(unix)]
	#[test]
	fn skips_symlinks_and_rejects_replaced_ancestors() {
		let root = tempfile::tempdir().unwrap();
		let outside = tempfile::tempdir().unwrap();
		let path = root.path().join("1.gma");
		fs::write(outside.path().join("keep"), b"keep").unwrap();
		std::os::unix::fs::symlink(outside.path().join("keep"), &path).unwrap();
		assert!(candidate(path, 1, Source::Cache, &HashSet::new()).unwrap().is_none());
		std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
		assert!(check_ancestors(&root.path().join("link/keep")).is_err());
	}
}
