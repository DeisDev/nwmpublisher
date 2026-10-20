use std::{
	collections::{BinaryHeap, HashMap, HashSet},
	fs::DirEntry,
	path::{Path, PathBuf},
	sync::{
		atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
		mpsc, Arc,
	},
	time::{Duration, SystemTime},
};

use parking_lot::{Mutex, RwLock, RwLockReadGuard};
use rayon::ThreadPool;
use serde::Serialize;
use steamworks::{ItemState, PublishedFileId};
use tauri::ipc::Channel;

use crate::{
	addon_cleaner::{self, CleanResult, CleanerError},
	appdata::InstalledAddonsSort,
	gma::{
		extract::{ExtractDestination, ExtractGMAMut, ExtractOptions},
		AddonType,
	},
	webview::Addon,
	GMAFile,
};

static DISCOVERY_POOL: std::sync::LazyLock<ThreadPool> = std::sync::LazyLock::new(|| thread_pool!(4));
const STEAM_TIMEOUT: Duration = Duration::from_secs(30);
static REMOVAL: Mutex<()> = Mutex::new(());
static REMOVAL_STOP: AtomicBool = AtomicBool::new(false);

#[repr(u8)]
enum Discovered {
	No = 0,
	Discovering = 1,
	Yes = 2,
}
impl From<u8> for Discovered {
	#[inline(always)]
	fn from(n: u8) -> Self {
		match n {
			0 => Discovered::No,
			1 => Discovered::Discovering,
			2 => Discovered::Yes,
			_ => unreachable!("invalid addon discovery state {n}"),
		}
	}
}
impl From<Discovered> for u8 {
	#[inline(always)]
	fn from(val: Discovered) -> Self {
		val as u8
	}
}

/// Only one caller runs `refresh`; the others wait for it. `refresh` leaves the state at `No` when the game is missing.
fn await_discovery(state: &AtomicU8, refresh: impl FnOnce()) {
	loop {
		match state.load(Ordering::Acquire).into() {
			Discovered::Yes => return,
			Discovered::No => {
				if state.compare_exchange(Discovered::No.into(), Discovered::Discovering.into(), Ordering::AcqRel, Ordering::Acquire).is_ok() {
					return refresh();
				}
			}
			Discovered::Discovering => sleep_ms!(25),
		}
	}
}

#[derive(Debug)]
pub struct GameAddons {
	discovered: AtomicU8,
	/// Bumped whenever the caches are cleared, so a scan that started earlier is not marked current.
	generation: AtomicU64,
	paths: RwLock<HashMap<PathBuf, Arc<Addon>>>,
	pages: RwLock<Vec<Arc<Addon>>>,
	external: RwLock<HashMap<PathBuf, Option<Arc<Addon>>>>,
	/// Steam removes an unsubscribed item's copy only after the game closes, so it is hidden for the rest of this run.
	unsubscribed: RwLock<HashSet<PublishedFileId>>,
}

impl GameAddons {
	pub fn init() -> GameAddons {
		GameAddons {
			discovered: AtomicU8::new(Discovered::No.into()),
			generation: AtomicU64::new(0),
			paths: RwLock::new(HashMap::new()),
			pages: RwLock::new(Vec::new()),
			external: RwLock::new(HashMap::new()),
			unsubscribed: RwLock::new(HashSet::new()),
		}
	}

	fn gma_check(entry: Result<DirEntry, std::io::Error>) -> Option<(PathBuf, String)> {
		let path = entry.ok()?.path();
		if !path.is_file() {
			return None;
		}

		let extension = path.extension()?.to_string_lossy().to_lowercase();
		if extension != "gma" {
			return None;
		}

		let file_name = path.file_name()?.to_string_lossy().to_string();

		Some((path, file_name[..(file_name.len() - 4)].to_owned()))
	}

	pub fn get_ws_id<S: AsRef<str>>(_file_name: S) -> Option<PublishedFileId> {
		let _file_name = _file_name.as_ref();
		let file_name = _file_name.strip_prefix("ds_").unwrap_or(_file_name);

		if let Ok(id) = str::parse::<u64>(file_name) {
			return Some(PublishedFileId(id));
		}

		let id = GameAddons::extract_suffix_ws_id(file_name);
		if id == 0 {
			None
		} else {
			Some(PublishedFileId(id))
		}
	}

	fn extract_suffix_ws_id<S: AsRef<str>>(file_name: S) -> u64 {
		let file_name = file_name.as_ref();
		let digits = file_name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
		file_name[digits..].parse().unwrap_or(0)
	}

	/// Workshop content is stored in `workshop/content/4000/<id>/`.
	pub fn workshop_content_id(path: &Path) -> Option<PublishedFileId> {
		let folder = path.parent()?;
		let app = folder.parent()?;
		if app.file_name()? != "4000" || app.parent()?.file_name()? != "content" { return None; }
		folder.file_name()?.to_str()?.parse().ok().filter(|id| *id != 0).map(PublishedFileId)
	}

	fn get_workshop_content_dir<P: AsRef<Path>>(gmod: P) -> Option<PathBuf> {
		Some(gmod.as_ref().parent()?.parent()?.join("workshop/content/4000"))
	}

	pub fn refresh(&self) {
		self.discovered.store(Discovered::Discovering.into(), Ordering::Release);
		let generation = self.generation.load(Ordering::Acquire);

		let mut gmod = if let Some(gmod) = app_data!().gmod_dir() {
			gmod
		} else {
			*self.paths.write() = HashMap::new();
			*self.pages.write() = Vec::new();

			self.discovered.store(Discovered::No.into(), Ordering::Release);
			return;
		};

		let workshop_content_dir = GameAddons::get_workshop_content_dir(&gmod);

		let addons_dir = gmod.join("GarrysMod/addons");

		gmod.push("GarrysMod/cache/workshop");
		let cache_dir = gmod;

		let (tx_metadata, rx_metadata) = mpsc::channel();
		let (tx, rx) = mpsc::channel();

		if let Some(workshop_content_dir) = workshop_content_dir {
			let tx_workshop_content_metadata = tx_metadata.clone();
			DISCOVERY_POOL.spawn(move || {
				let addons = match workshop_content_dir.read_dir() {
					Ok(addons) => addons,
					Err(_) => return,
				};

				for (id, mut read_dir) in addons.filter_map(|entry| {
					entry.ok().and_then(|entry| {
						if entry.file_type().ok()?.is_dir() {
							let read_dir = entry.path().read_dir().ok()?;
							let id = entry.file_name().to_string_lossy().parse::<u64>().ok()?;
							Some((id, read_dir))
						} else {
							None
						}
					})
				}) {
					if let Some(gma_path) = {
						read_dir.find_map(|entry| {
							entry.ok().and_then(|entry| {
								if entry.file_type().ok()?.is_file() && entry.path().extension().and_then(|x| x.to_str())? == "gma" {
									Some(entry.path().to_path_buf())
								} else {
									None
								}
							})
						})
					} {
						tx_workshop_content_metadata
							.send((gma_path, if id == 0 { None } else { Some(PublishedFileId(id)) }))
							.unwrap();
					}
				}
			});
		}

		let tx_addons_metadata = tx_metadata.clone();
		DISCOVERY_POOL.spawn(move || {
			let addons = match addons_dir.read_dir() {
				Ok(addons) => addons,
				Err(_) => return,
			};

			for (path, _file_name) in addons.filter_map(GameAddons::gma_check) {
				let file_name = _file_name.strip_prefix("ds_").unwrap_or(&_file_name);
				let id = GameAddons::extract_suffix_ws_id(file_name);

				tx_addons_metadata
					.send((path, if id == 0 { None } else { Some(PublishedFileId(id)) }))
					.unwrap();
			}
		});

		let tx_cache_metadata = tx_metadata;
		DISCOVERY_POOL.spawn(move || {
			let cache = match cache_dir.read_dir() {
				Ok(cache) => cache,
				Err(_) => return,
			};

			for (path, file_name) in cache.filter_map(GameAddons::gma_check) {
				let id = match str::parse::<u64>(&file_name) {
					Ok(id) => Some(PublishedFileId(id)),
					Err(_ok) => None,
				};

				tx_cache_metadata.send((path, id)).unwrap();
			}
		});

		DISCOVERY_POOL.spawn(move || {
			while let Ok((path, id)) = rx_metadata.recv() {
				let mut gma = match GMAFile::open(&path) {
					Ok(gma) => gma,
					Err(_) => continue,
				};

				if let Some(id) = id {
					gma.set_ws_id(id);
				}

				ignore! { gma.metadata() };

				tx.send(gma).unwrap();
			}
		});

		let ids = {
			let mut ids = Vec::new();

			let mut pages = self.pages.write();
			*pages = Vec::new();

			let mut paths = self.paths.write();
			*paths = HashMap::new();

			let mut pages_heap = BinaryHeap::new();

			while let Ok(mut gma) = rx.recv() {
				let modified = gma
					.path
					.metadata()
					.and_then(|metadata| {
						metadata
							.modified()
							.map(|x| Some(x.duration_since(SystemTime::UNIX_EPOCH).map(|dur| dur.as_secs()).unwrap_or(0)))
					})
					.unwrap_or(None);
				gma.modified = modified;

				if let Some(id) = gma.id {
					ids.push(id);
				}

				let path = gma.path.to_owned();
				let gma: Arc<Addon> = Arc::new(gma.into());

				pages_heap.push(gma.clone());
				paths.insert(path, gma);
			}

			*pages = pages_heap.into_sorted_vec();

			search!().add_bulk(&pages);

			println!("Discovered {} addons", paths.len());

			// Set while the maps are still locked, so a concurrent free_caches cannot be overwritten.
			let current = self.generation.load(Ordering::Acquire) == generation;
			self.discovered.store(if current { Discovered::Yes } else { Discovered::No }.into(), Ordering::Release);
			current.then_some(ids)
		};
		let Some(ids) = ids else { return };

		let (sort, types) = {
			let settings = app_data!().settings.read();
			(settings.installed_addons_sort, settings.installed_addons_types.clone())
		};
		browse_installed_addons(1, sort, types); // Download the first page first
		steam!().fetch_workshop_items(ids); // Download the rest in the background too
	}

	pub fn discover_addons(&self) {
		main_thread_forbidden!();
		await_discovery(&self.discovered, || self.refresh());
	}

	/// Paths sent to the webview are canonical, while discovery keeps the paths it read.
	pub fn from_path<P: AsRef<Path>>(&self, path: P) -> Option<Arc<Addon>> {
		self.discover_addons();
		let paths = self.paths.read();
		if let Some(addon) = paths.get(path.as_ref()) {
			return Some(addon.clone());
		}
		let path = dunce::canonicalize(path).ok()?;
		let name = path.file_name()?;
		paths
			.iter()
			.find(|(candidate, _)| candidate.file_name() == Some(name) && dunce::canonicalize(candidate).is_ok_and(|candidate| candidate == path))
			.map(|(_, addon)| addon.clone())
	}

	pub fn get_addons(&self) -> RwLockReadGuard<'_, Vec<Arc<Addon>>> {
		self.discover_addons();
		self.pages.read()
	}
}

/// Keeps the newest-first discovery order, grouping by type when sorting by type.
fn select_installed_addons(addons: &[Arc<Addon>], sort: InstalledAddonsSort, types: &[AddonType], unsubscribed: &HashSet<PublishedFileId>) -> Vec<Arc<Addon>> {
	let addon_type = |addon: &Arc<Addon>| AddonType::of(addon.installed().metadata.as_ref());
	let pending_removal = |addon: &Arc<Addon>| GameAddons::workshop_content_id(&addon.installed().path).is_some_and(|id| unsubscribed.contains(&id));
	let mut selected: Vec<_> = addons
		.iter()
		.filter(|addon| types.contains(&addon_type(addon)) && !pending_removal(addon))
		.cloned()
		.collect();
	if sort == InstalledAddonsSort::Type {
		selected.sort_by_key(addon_type);
	}
	selected
}

#[tauri::command]
pub fn browse_installed_addons(page: u32, sort: InstalledAddonsSort, types: Vec<AddonType>) -> (usize, Vec<Arc<Addon>>) {
	let unsubscribed = game_addons!().unsubscribed.read().clone();
	let selected = select_installed_addons(&game_addons!().get_addons(), sort, &types, &unsubscribed);
	let start = ((page.max(1) - 1) as usize) * crate::steam::RESULTS_PER_PAGE;
	let page: Vec<_> = selected.iter().skip(start).take(crate::steam::RESULTS_PER_PAGE).cloned().collect();
	steam!().fetch_workshop_items(page.iter().filter_map(|addon| addon.installed().id).collect());
	(selected.len(), page)
}

#[tauri::command]
pub fn browse_all_installed_addons(sort: InstalledAddonsSort, types: Vec<AddonType>) -> Vec<Arc<Addon>> {
	let unsubscribed = game_addons!().unsubscribed.read().clone();
	select_installed_addons(&game_addons!().get_addons(), sort, &types, &unsubscribed)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Removal {
	Unsubscribe,
	Delete,
}

/// Steam downloads subscribed items again and restores the files of items it still tracks, so those are not deleted.
fn removal(steam_managed: bool, state: Option<ItemState>) -> Option<Removal> {
	match state {
		Some(state) if state.contains(ItemState::SUBSCRIBED) => Some(Removal::Unsubscribe),
		Some(state) if state.intersects(ItemState::DOWNLOADING | ItemState::DOWNLOAD_PENDING) || (steam_managed && !state.is_empty()) => None,
		None if steam_managed => None,
		_ => Some(Removal::Delete),
	}
}

fn addon_removal(gma: &GMAFile) -> Option<Removal> {
	let state = match gma.id {
		Some(id) if steam!().connected() => Some(steam!().client().ugc().item_state(id)),
		Some(_) => None,
		None => Some(ItemState::empty()),
	};
	removal(GameAddons::workshop_content_id(&gma.path).is_some(), state)
}

fn installed_addon(path: &Path) -> Result<Arc<Addon>, CleanerError> {
	game_addons!().from_path(path).ok_or_else(|| CleanerError::new("addon_removal_missing"))
}

#[tauri::command]
pub fn installed_addon_removal(paths: Vec<PathBuf>) -> Vec<Option<Removal>> {
	game_addons!().discover_addons();
	let known = game_addons!().paths.read();
	let mut canonical: Option<HashMap<PathBuf, Arc<Addon>>> = None;
	paths
		.iter()
		.map(|path| {
			let addon = known.get(path).cloned().or_else(|| {
				let canonical = canonical.get_or_insert_with(|| known.iter().filter_map(|(path, addon)| Some((dunce::canonicalize(path).ok()?, addon.clone()))).collect());
				canonical.get(&dunce::canonicalize(path).ok()?).cloned()
			})?;
			addon_removal(addon.installed())
		})
		.collect()
}

#[derive(Clone, Serialize)]
pub struct RemovalProgress {
	completed: usize,
	total: usize,
}

#[derive(Serialize)]
pub struct RemovalOutcome {
	path: PathBuf,
	error: Option<CleanerError>,
	files: Option<CleanResult>,
}

/// Removes addons one at a time until done or stopped. Items left after a stop have no outcome.
fn run_removal(paths: Vec<PathBuf>, on_progress: &Channel<RemovalProgress>, mut remove: impl FnMut(&Path) -> Result<Option<CleanResult>, CleanerError>) -> Vec<RemovalOutcome> {
	let total = paths.len();
	let mut progress = Some(on_progress);
	let mut outcomes = Vec::with_capacity(total);
	for path in paths {
		if REMOVAL_STOP.load(Ordering::Acquire) {
			break;
		}
		outcomes.push(match remove(&path) {
			Ok(files) => RemovalOutcome { path, error: None, files },
			Err(error) => RemovalOutcome { path, error: Some(error), files: None },
		});
		if let Some(channel) = progress {
			if let Err(error) = channel.send(RemovalProgress { completed: outcomes.len(), total }) {
				eprintln!("Addon removal progress delivery failed: {error}");
				progress = None;
			}
		}
	}
	outcomes
}

#[tauri::command]
pub fn stop_installed_addon_removal() {
	REMOVAL_STOP.store(true, Ordering::Release);
}

fn unsubscribe_addon(path: &Path, gmod: Option<&Path>, permanent: bool, unsubscribed: &mut HashSet<PublishedFileId>) -> Result<(PublishedFileId, CleanResult), CleanerError> {
	let addon = installed_addon(path)?;
	let gma = addon.installed();
	let id = match (addon_removal(gma), gma.id) {
		(Some(Removal::Unsubscribe), Some(id)) => id,
		_ => return Err(CleanerError::new("addon_unsubscribe_not_subscribed")),
	};
	let files = match gmod {
		Some(gmod) => addon_cleaner::item_archives(gmod, id, &gma.path)?,
		None => Vec::new(),
	};
	// Copies of one item share its ID, and Steam still reports it as subscribed right after unsubscribing.
	if !unsubscribed.contains(&id) {
		let (sender, receiver) = mpsc::sync_channel(1);
		steam!().client().ugc().unsubscribe_item(id, move |result| {
			let _ = sender.send(result);
		});
		receiver
			.recv_timeout(STEAM_TIMEOUT)
			.map_err(|_| CleanerError::new("addon_unsubscribe_timeout"))?
			.map_err(|error| CleanerError::detail("addon_unsubscribe_error", error))?;
		unsubscribed.insert(id);
		game_addons!().unsubscribed.write().insert(id);
	}
	Ok((id, addon_cleaner::remove_archives(files, permanent)))
}

#[tauri::command]
pub async fn unsubscribe_installed_addons(paths: Vec<PathBuf>, delete_files: bool, permanent: bool, on_progress: Channel<RemovalProgress>) -> Result<Vec<RemovalOutcome>, CleanerError> {
	tauri::async_runtime::spawn_blocking(move || {
		let _removal = REMOVAL.try_lock().ok_or_else(|| CleanerError::new("addon_removal_busy"))?;
		REMOVAL_STOP.store(false, Ordering::Release);
		if !steam!().connected() {
			return Err(CleanerError::new("addon_unsubscribe_offline"));
		}
		let gmod = if delete_files { Some(app_data!().gmod_dir().ok_or_else(|| CleanerError::new("cleaner_error_gmod"))?) } else { None };
		let mut unsubscribed = HashSet::new();
		let outcomes = run_removal(paths, &on_progress, |path| match unsubscribe_addon(path, gmod.as_deref(), permanent, &mut unsubscribed) {
			Ok((id, result)) => {
				println!("Unsubscribed from Workshop item {}", id.0);
				if delete_files {
					println!("{} {} files of Workshop item {}", if permanent { "Deleted" } else { "Moved to Trash" }, result.removed.len(), id.0);
				}
				for failure in &result.failed {
					wprintln!("Kept {} after unsubscribing: {}", failure.path.display(), failure.error);
				}
				Ok(Some(result))
			}
			Err(error) => {
				eprintln!("Could not unsubscribe from an installed addon: {}", error);
				Err(error)
			}
		});
		if outcomes.iter().any(|outcome| outcome.files.as_ref().is_some_and(|files| !files.removed.is_empty())) {
			crate::commands::free_caches();
		}
		if outcomes.iter().any(|outcome| outcome.error.is_none()) {
			webview_emit!("InstalledAddonsRefreshed");
		}
		Ok(outcomes)
	})
	.await
	.map_err(|error| CleanerError::detail("cleaner_error_task", error))?
}

fn delete_addon(path: &Path, permanent: bool) -> Result<PathBuf, CleanerError> {
	let addon = installed_addon(path)?;
	let gma = addon.installed();
	match addon_removal(gma) {
		Some(Removal::Delete) => {}
		Some(Removal::Unsubscribe) => return Err(CleanerError::new("cleaner_error_subscribed")),
		None => return Err(CleanerError::new("cleaner_error_steam")),
	}
	let path = gma.path.clone();
	let metadata = std::fs::symlink_metadata(&path).map_err(|error| CleanerError::io(&path, error))?;
	if !metadata.is_file() || addon_cleaner::is_link(&metadata) {
		return Err(CleanerError::new("addon_removal_changed"));
	}
	addon_cleaner::remove_file(&path, permanent)?;
	if GameAddons::workshop_content_id(&path).is_some() {
		if let Err(error) = addon_cleaner::remove_empty_folder(path.parent().unwrap()) {
			wprintln!("Could not remove the empty Workshop folder of {}: {}", path.display(), error);
		}
	}
	Ok(path)
}

#[tauri::command]
pub async fn delete_installed_addons(paths: Vec<PathBuf>, permanent: bool, on_progress: Channel<RemovalProgress>) -> Result<Vec<RemovalOutcome>, CleanerError> {
	tauri::async_runtime::spawn_blocking(move || {
		let _removal = REMOVAL.try_lock().ok_or_else(|| CleanerError::new("addon_removal_busy"))?;
		REMOVAL_STOP.store(false, Ordering::Release);
		let outcomes = run_removal(paths, &on_progress, |path| match delete_addon(path, permanent) {
			Ok(path) => {
				println!("{} installed addon {}", if permanent { "Deleted" } else { "Moved to Trash" }, path.display());
				Ok(None)
			}
			Err(error) => {
				eprintln!("Could not delete an installed addon: {}", error);
				Err(error)
			}
		});
		if outcomes.iter().any(|outcome| outcome.error.is_none()) {
			crate::commands::free_caches();
			webview_emit!("InstalledAddonsRefreshed");
		}
		Ok(outcomes)
	})
	.await
	.map_err(|error| CleanerError::detail("cleaner_error_task", error))?
}

#[tauri::command]
pub fn get_installed_addon(path: PathBuf) -> Option<Arc<Addon>> {
	game_addons!().discover_addons();

	if let Some(cached) = game_addons!().external.read().get(&path) {
		return cached.clone();
	}

	if path.is_absolute() && path.is_file() && crate::path::has_extension(&path, "gma") {
		match GMAFile::open(&path) {
			Ok(mut gma) => {
				if let Some(id) = GameAddons::get_ws_id(path.file_name().unwrap().to_string_lossy()) {
					gma.set_ws_id(id);
				}

				ignore! { gma.metadata() };

				let gma = Arc::new(Addon::Installed(gma));
				game_addons!().external.write().insert(path, Some(gma.clone()));
				Some(gma)
			}
			Err(_) => {
				game_addons!().external.write().insert(path, None);
				None
			}
		}
	} else {
		None
	}
}

#[tauri::command]
pub fn downloader_extract_gmas(paths: Vec<PathBuf>) {
	let destination = app_data!().settings.read().extract_destination.clone();
	let options = ExtractOptions::from_settings();
	for path in paths.into_iter() {
		if match path.extension() {
			Some(extension) => extension.to_string_lossy().eq_ignore_ascii_case("gma"),
			None => false,
		} {
			let transaction = crate::transactions::new_extraction();
			let gma = GMAFile::open(&path);
			transaction.context(serde_json::json!({ "kind": "extract", "sourcePath": path, "fileName": path.file_name().map(|name| name.to_string_lossy()), "workshopId": gma.as_ref().ok().and_then(|gma| gma.id) }));
			webview_emit!(
				"ExtractionStarted",
				(
					transaction.id,
					Some(path.clone()),
					path.file_name().map(|name| name.to_string_lossy().to_string()),
					gma.as_ref().ok().and_then(|gma| gma.id)
				)
			);
			let destination = destination.clone();
			let mut options = options.clone();
			crate::gma::extract::THREAD_POOL.spawn(move || match gma {
				Ok(mut gma) => {
					transaction.data((turbonone!(), gma.size));
					if !crate::steam::item_info::attach(&mut options, gma.inferred_ws_id(), true, None, &transaction) { return transaction.cancelled(); }
					let _ = gma.extract(destination.clone(), &transaction, false, true, &options);
				}
				Err(error) => transaction.error(error.to_string(), turbonone!()),
			});
		}
	}
}

/// Extracting into the chosen folder itself would merge every addon into one folder.
fn separate_folders(destination: ExtractDestination) -> ExtractDestination {
	match destination {
		ExtractDestination::Directory(path) => ExtractDestination::NamedDirectory(path),
		destination => destination,
	}
}

/// Queues installed addons in the Downloader's extraction list. Returns how many were queued; copies of one Workshop item are extracted once.
#[tauri::command]
pub fn extract_installed_addons(paths: Vec<PathBuf>, destination: ExtractDestination) -> usize {
	let destination = separate_folders(destination);
	let options = ExtractOptions::from_settings();
	let mut ids = HashSet::new();
	let mut queued = 0;
	for path in paths {
		let Some(addon) = game_addons!().from_path(&path) else { continue };
		let id = addon.installed().id;
		if id.is_some_and(|id| !ids.insert(id)) {
			continue;
		}
		let file_name = path.file_name().map(|name| name.to_string_lossy().to_string());
		let transaction = crate::transactions::new_extraction();
		transaction.context(serde_json::json!({ "kind": "extract", "sourcePath": path, "fileName": file_name, "workshopId": id }));
		webview_emit!("ExtractionStarted", (transaction.id, Some(path.clone()), file_name, id));
		let destination = destination.clone();
		let mut options = options.clone();
		crate::gma::extract::THREAD_POOL.spawn(move || match GMAFile::open(&path) {
			Ok(mut gma) => {
				if let Some(id) = id {
					gma.set_ws_id(id);
				}
				transaction.data((turbonone!(), gma.size));
				if !crate::steam::item_info::attach(&mut options, gma.inferred_ws_id(), true, None, &transaction) { return transaction.cancelled(); }
				let _ = gma.extract(destination, &transaction, false, true, &options);
			}
			Err(error) => transaction.error(error.to_string(), turbonone!()),
		});
		queued += 1;
	}
	queued
}

pub fn free_caches() {
	let game_addons = crate::game_addons!();
	game_addons.external.write().clear();
	let mut pages = game_addons.pages.write();
	let mut paths = game_addons.paths.write();
	*pages = Vec::new();
	*paths = HashMap::new();
	game_addons.generation.fetch_add(1, Ordering::AcqRel);
	game_addons.discovered.store(Discovered::No.into(), Ordering::Release);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn workshop_ids_are_read_from_file_names_and_content_folders() {
		for (name, id) in [("123456", Some(123456)), ("ds_789", Some(789)), ("my_addon_123456", Some(123456)), ("addon2_0042", Some(42)), ("addon", None), ("addon_99999999999999999999", None)] {
			assert_eq!(GameAddons::get_ws_id(name), id.map(PublishedFileId), "{name}");
		}
		let content = Path::new("steamapps/workshop/content/4000");
		assert_eq!(GameAddons::workshop_content_id(&content.join("123/addon.gma")), Some(PublishedFileId(123)));
		for path in [content.join("abc/addon.gma"), content.join("0/addon.gma"), Path::new("content/4001/123/addon.gma").to_owned(), Path::new("downloads/123/addon.gma").to_owned()] {
			assert_eq!(GameAddons::workshop_content_id(&path), None, "{}", path.display());
		}
	}

	#[test]
	fn discovery_runs_one_refresh_and_waits_for_it() {
		let state = Arc::new(AtomicU8::new(Discovered::No.into()));
		let refreshes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
		let callers: Vec<_> = (0..4)
			.map(|_| {
				let (state, refreshes) = (state.clone(), refreshes.clone());
				std::thread::spawn(move || {
					await_discovery(&state, || {
						refreshes.fetch_add(1, Ordering::SeqCst);
						sleep_ms!(50);
						state.store(Discovered::Yes.into(), Ordering::Release);
					})
				})
			})
			.collect();
		for caller in callers {
			caller.join().unwrap();
		}
		assert_eq!(refreshes.load(Ordering::SeqCst), 1);

		let missing_game = AtomicU8::new(Discovered::No.into());
		await_discovery(&missing_game, || missing_game.store(Discovered::No.into(), Ordering::Release));
		assert_eq!(missing_game.load(Ordering::Acquire), u8::from(Discovered::No));
	}

	#[test]
	fn bulk_extraction_gives_each_addon_its_own_folder() {
		let folder = PathBuf::from("extracted");
		assert!(matches!(separate_folders(ExtractDestination::Directory(folder.clone())), ExtractDestination::NamedDirectory(path) if path == folder));
		assert!(matches!(separate_folders(ExtractDestination::NamedDirectory(folder.clone())), ExtractDestination::NamedDirectory(path) if path == folder));
		assert!(matches!(separate_folders(ExtractDestination::Temp), ExtractDestination::Temp));
	}

	#[test]
	fn removal_is_offered_only_when_steam_would_not_restore_the_file() {
		assert_eq!(removal(false, Some(ItemState::SUBSCRIBED | ItemState::INSTALLED)), Some(Removal::Unsubscribe));
		assert_eq!(removal(true, Some(ItemState::SUBSCRIBED)), Some(Removal::Unsubscribe));
		assert_eq!(removal(false, Some(ItemState::empty())), Some(Removal::Delete));
		assert_eq!(removal(true, Some(ItemState::empty())), Some(Removal::Delete));
		assert_eq!(removal(false, Some(ItemState::INSTALLED)), Some(Removal::Delete));
		assert_eq!(removal(true, Some(ItemState::INSTALLED)), None);
		assert_eq!(removal(false, Some(ItemState::DOWNLOADING)), None);
		assert_eq!(removal(false, Some(ItemState::DOWNLOAD_PENDING)), None);
		assert_eq!(removal(false, None), Some(Removal::Delete));
		assert_eq!(removal(true, None), None);
	}

	#[test]
	fn installed_addons_filter_by_type_and_group_without_reordering_within_types() {
		let addon = |name: &str, addon_type: Option<&str>| -> Arc<Addon> {
			Arc::new(Addon::Installed(GMAFile {
				path: PathBuf::from(name),
				size: 0,
				id: None,
				metadata: addon_type.map(|addon_type| crate::gma::GMAMetadata::Standard { title: name.into(), addon_type: addon_type.into(), tags: Vec::new(), ignore: Vec::new() }),
				entries: None,
				pointers: Default::default(),
				version: 3,
				extracted_name: name.into(),
				modified: None,
				membuffer: None,
				spool: None,
			}))
		};
		let addons = [addon("weapon1", Some("weapon")), addon("map1", Some("map")), addon("legacy", None), addon("weapon2", Some("Weapon")), addon("map2", Some("map"))];
		let names = |selected: Vec<Arc<Addon>>| selected.iter().map(|addon| addon.installed().extracted_name.clone()).collect::<Vec<_>>();
		let none = HashSet::new();
		assert_eq!(names(select_installed_addons(&addons, InstalledAddonsSort::Modified, &AddonType::ALL, &none)), ["weapon1", "map1", "legacy", "weapon2", "map2"]);
		assert_eq!(names(select_installed_addons(&addons, InstalledAddonsSort::Type, &AddonType::ALL, &none)), ["map1", "map2", "weapon1", "weapon2", "legacy"]);
		assert_eq!(names(select_installed_addons(&addons, InstalledAddonsSort::Type, &[AddonType::Other, AddonType::Weapon], &none)), ["weapon1", "weapon2", "legacy"]);
		assert!(select_installed_addons(&addons, InstalledAddonsSort::Modified, &[], &none).is_empty());

		let steam_copy = "steamapps/workshop/content/4000/10/item.gma";
		let addons = [addon(steam_copy, Some("map")), addon("garrysmod/cache/workshop/10.gma", Some("map")), addon("steamapps/workshop/content/4000/20/item.gma", Some("map"))];
		assert_eq!(names(select_installed_addons(&addons, InstalledAddonsSort::Modified, &AddonType::ALL, &none)).len(), 3);
		assert_eq!(
			names(select_installed_addons(&addons, InstalledAddonsSort::Modified, &AddonType::ALL, &HashSet::from([PublishedFileId(10)]))),
			["garrysmod/cache/workshop/10.gma", "steamapps/workshop/content/4000/20/item.gma"]
		);
	}
}
