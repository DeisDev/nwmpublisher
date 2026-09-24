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
use steamworks::{AppIDs, ItemState, PublishedFileId, SteamId, UGCType, UserList, UserListOrder};
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
	fn new(key: &'static str) -> Self {
		Self { key, detail: String::new() }
	}
	fn detail(key: &'static str, detail: impl std::fmt::Display) -> Self {
		Self {
			key,
			detail: detail.to_string(),
		}
	}
	fn io(path: &Path, error: impl std::fmt::Display) -> Self {
		Self::detail("cleaner_error_file", format!("{}: {}", path.display(), error))
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
	#[serde(skip)]
	modified: SystemTime,
	#[serde(skip)]
	created: Option<SystemTime>,
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
	path: PathBuf,
	error: CleanerError,
}

#[derive(Default, Serialize)]
pub struct CleanResult {
	removed: Vec<PathBuf>,
	failed: Vec<FileFailure>,
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

fn append_page(ids: &mut HashSet<PublishedFileId>, total: u32, expected: &mut Option<u32>, page: Vec<PublishedFileId>) -> Result<bool, CleanerError> {
	if expected.is_some_and(|previous| previous != total) || (page.is_empty() && ids.len() < total as usize) {
		return Err(CleanerError::new("cleaner_error_subscriptions"));
	}
	*expected = Some(total);
	for id in page {
		if id.0 == 0 || !ids.insert(id) {
			return Err(CleanerError::new("cleaner_error_subscriptions"));
		}
	}
	if ids.len() > total as usize {
		return Err(CleanerError::new("cleaner_error_subscriptions"));
	}
	Ok(ids.len() == total as usize)
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
				.and_then(|results| {
					let ids = (0..results.returned_results())
						.map(|index| {
							results
								.get(index)
								.map(|item| item.published_file_id)
								.ok_or_else(|| CleanerError::new("cleaner_error_subscriptions"))
						})
						.collect::<Result<Vec<_>, _>>()?;
					Ok((results.total_results(), ids))
				});
			let _ = sender.send((page, result));
		});
	Ok(())
}

fn subscriptions(owner: SteamId, progress: &mut Progress) -> Result<HashSet<PublishedFileId>, CleanerError> {
	progress.stage("subscriptions", None);
	let mut ids = HashSet::new();
	let mut expected = None;
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
		let page_size = steamworks::RESULTS_PER_PAGE;
		if page_ids.len() != total.saturating_sub((page - 1) * page_size).min(page_size) as usize {
			return Err(CleanerError::new("cleaner_error_subscriptions"));
		}
		append_page(&mut ids, total, &mut expected, page_ids)?;
		progress.state.completed = ids.len();
		progress.state.total = Some(total as usize);
		progress.send(true);
		let pages = total.div_ceil(page_size);
		while pending.len() < SUBSCRIPTION_CONCURRENCY && next_page <= pages {
			pending.insert(next_page, Instant::now());
			request_subscription_page(owner, next_page, sender.clone())?;
			next_page += 1;
		}
	}
	if expected != Some(ids.len() as u32) {
		return Err(CleanerError::new("cleaner_error_subscriptions"));
	}
	if account()? != owner {
		return Err(CleanerError::new("cleaner_error_account"));
	}
	// Keep local subscriptions too, including items missing from a server query.
	ids.extend(steam!().client().ugc().subscribed_items());
	Ok(ids)
}

fn is_link(metadata: &Metadata) -> bool {
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
	Ok(Some(Candidate {
		file_name: path.file_name().unwrap().to_string_lossy().into_owned(),
		source,
		modified: metadata.modified().map_err(|error| CleanerError::io(&path, error))?,
		created: metadata.created().ok(),
		size: metadata.len(),
		path,
		workshop_id: id.to_string(),
	}))
}

fn discover(gmod: &Path, subscribed: &HashSet<PublishedFileId>, progress: &mut Progress) -> Result<Vec<Candidate>, CleanerError> {
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
			// The content root was checked above and this child is a regular directory.
			for path in read_entries(&folder)? {
				if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("gma")) {
					if let Some(file) = candidate(path.clone(), id, Source::Workshop, subscribed)? {
						files.push(file);
					}
				}
				progress.checking(&path, files.len());
			}
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
	if !metadata.is_file()
		|| metadata.len() != file.size
		|| metadata.modified().map_err(|error| CleanerError::io(&file.path, error))? != file.modified
		|| metadata.created().ok() != file.created
	{
		return Err(CleanerError::new("cleaner_error_changed"));
	}
	Ok(())
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
	mut authorize: impl FnMut(&Candidate) -> Result<(), CleanerError>,
) -> CleanResult {
	progress.stage(if permanent { "deleting" } else { "recycling" }, Some(files.len()));
	let mut result = CleanResult::default();
	for file in files {
		progress.state.path = Some(file.path.clone());
		progress.send(progress.state.completed == 0);
		let removal = authorize(&file).and_then(|_| unchanged(&file)).and_then(|_| {
			if permanent {
				fs::remove_file(&file.path).map_err(|error| CleanerError::io(&file.path, error))
			} else {
				trash::delete(&file.path).map_err(|error| CleanerError::io(&file.path, error))?;
				// Some shell operations can report success even when the user cancels.
				match fs::symlink_metadata(&file.path) {
					Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
					Err(error) => Err(CleanerError::io(&file.path, error)),
					Ok(_) => Err(CleanerError::new("cleaner_error_retained")),
				}
			}
		});
		match removal {
			Ok(()) => result.removed.push(file.path),
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
	tauri::async_runtime::spawn_blocking(move || {
		let mut progress = Progress::new(on_progress);
		let mut scan = SCAN.try_lock().ok_or_else(|| CleanerError::new("cleaner_error_busy"))?;
		*scan = None;
		let owner = account()?;
		let subscribed = subscriptions(owner, &mut progress)?;
		progress.stage("locating", None);
		let gmod = app_data!().gmod_dir().ok_or_else(|| CleanerError::new("cleaner_error_gmod"))?;
		let files = discover(&gmod, &subscribed, &mut progress)?;
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
	.map_err(|error| CleanerError::detail("cleaner_error_task", error))?
}

#[tauri::command]
pub async fn clean_addons(
	scan_id: String,
	selected: Vec<usize>,
	permanent: bool,
	on_progress: Channel<CleanerProgress>,
) -> Result<CleanResult, CleanerError> {
	tauri::async_runtime::spawn_blocking(move || {
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
		let result = remove_files(files, permanent, &mut progress, |file| {
			if account()? != owner {
				return Err(CleanerError::new("cleaner_error_account"));
			}
			let id = PublishedFileId(file.workshop_id.parse().unwrap());
			let state = steam!().client().ugc().item_state(id);
			if subscribed.contains(&id) || state.intersects(ItemState::SUBSCRIBED | ItemState::DOWNLOADING | ItemState::DOWNLOAD_PENDING) {
				return Err(CleanerError::new("cleaner_error_subscribed"));
			}
			Ok(())
		});
		*scan = None;
		if !result.removed.is_empty() {
			crate::commands::free_caches();
			webview_emit!("InstalledAddonsRefreshed");
		}
		Ok(result)
	})
	.await
	.map_err(|error| CleanerError::detail("cleaner_error_task", error))?
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
	}

	#[test]
	fn incomplete_or_changing_subscription_pages_fail_closed() {
		let mut ids = HashSet::new();
		let mut total = None;
		assert!(!append_page(&mut ids, 2, &mut total, vec![PublishedFileId(1)]).unwrap());
		assert!(append_page(&mut ids, 2, &mut total, vec![]).is_err());
		assert!(append_page(&mut ids, 3, &mut total, vec![PublishedFileId(2)]).is_err());
		assert!(append_page(&mut ids, 2, &mut total, vec![PublishedFileId(1)]).is_err());
		assert!(append_page(&mut ids, 2, &mut total, vec![PublishedFileId(2)]).unwrap());
		assert!(append_page(&mut HashSet::new(), 0, &mut None, vec![]).unwrap());
	}

	#[test]
	fn subscription_pages_can_complete_out_of_order_without_losing_ids() {
		let mut ids = HashSet::new();
		let mut total = None;
		for page in [1, 4, 2, 3] {
			let start = (page - 1) * 50 + 1;
			let complete = append_page(&mut ids, 200, &mut total, (start..start + 50).map(PublishedFileId).collect()).unwrap();
			assert_eq!(complete, page == 3);
		}
		assert_eq!(ids, (1..=200).map(PublishedFileId).collect());
	}

	#[test]
	fn workshop_details_are_bounded_and_duplicate_copies_share_one_lookup() {
		let file = Candidate {
			path: PathBuf::from("123.gma"),
			file_name: "123.gma".into(),
			source: Source::Cache,
			workshop_id: "123".into(),
			size: 1,
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
			"steamapps/workshop/content/4000/30/keep.txt",
			"steamapps/workshop/content/4000/30/source/nested.gma",
		] {
			let path = root_path.join(path);
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(path, b"test").unwrap();
		}
		let events = std::sync::Arc::new(Mutex::new(Vec::new()));
		let received = events.clone();
		let mut progress = Progress::new(Channel::new(move |body| {
			if let tauri::ipc::InvokeResponseBody::Json(json) = body {
				received.lock().push(serde_json::from_str::<serde_json::Value>(&json).unwrap());
			}
			Ok(())
		}));
		let files = discover(&gmod, &HashSet::from([PublishedFileId(40)]), &mut progress).unwrap();
		assert_eq!(files.len(), 3);
		assert_eq!(files.iter().find(|file| file.workshop_id == "30").unwrap().source, Source::Workshop);
		let events = events.lock();
		assert_eq!(events.last().unwrap()["found"], 3);
		assert!(events.last().unwrap()["completed"].as_u64().unwrap() > 3);
		for source in ["addons", "cache", "workshop"] {
			assert!(events.iter().any(|event| event["source"] == source));
		}
		assert_eq!(
			files.iter().map(|file| file.workshop_id.as_str()).collect::<HashSet<_>>(),
			HashSet::from(["10", "20", "30"])
		);
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
				Ok(())
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
