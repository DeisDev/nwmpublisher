use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::{
	cell::Cell,
	collections::HashMap,
	fs::{self, File, OpenOptions},
	io::{Seek, SeekFrom, Write},
	path::{Path, PathBuf},
	sync::atomic::{AtomicBool, Ordering},
};

use crate::{
	gma::{ExtractDestination, ExtractionOverwriteMode},
	steam::workshop::WorkshopVisibility,
	RwLockCow,
};

use crate::GMOD_APP_ID;
use lazy_static::lazy_static;
use parking_lot::{Mutex, RwLock, RwLockReadGuard};
use serde::{Deserialize, Serialize};
use steamworks::PublishedFileId;

lazy_static! {
	static ref USER_DATA_DIR: PathBuf = dirs_next::data_dir()
		.unwrap_or_else(|| std::env::current_exe().unwrap_or_else(|_| std::env::temp_dir()))
		.join("nwmpublisher");
	static ref APP_SETTINGS_PATH: PathBuf = dirs_next::config_dir()
		.unwrap_or_else(|| dirs_next::data_dir().unwrap_or_else(|| std::env::current_exe().unwrap_or_else(|_| std::env::temp_dir())))
		.join("nwmpublisher/settings.json");
	static ref LEGACY_APP_SETTINGS_PATH: PathBuf = dirs_next::config_dir()
		.unwrap_or_else(|| dirs_next::data_dir().unwrap_or_else(|| std::env::current_exe().unwrap_or_else(|_| std::env::temp_dir())))
		.join("gmpublisher/settings.json");
	static ref TEMP_DIR: PathBuf = std::env::temp_dir().join("nwmpublisher");
	static ref DOWNLOADS_DIR: Option<PathBuf> = dirs::download_dir();
	/// Whether nwmpublisher already had settings of its own when it started.
	/// Evaluated before anything can write settings, so it stays true for the whole run.
	static ref HAD_SETTINGS_FILE: bool = APP_SETTINGS_PATH.is_file();
	static ref SETTINGS_COMMIT: Mutex<()> = Mutex::new(());
	static ref SETTINGS_RECOVERY: RwLock<Option<String>> = RwLock::new(None);
}

/// Whether the legacy settings offer has been answered in this run.
/// Importing reloads the webview rather than restarting the process, so without this the
/// offer would keep coming back until the app was closed and opened again.
static MIGRATION_RESOLVED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub struct OpenCount(Cell<u32>);
unsafe impl Send for OpenCount {}
unsafe impl Sync for OpenCount {}
impl OpenCount {
	fn init() -> OpenCount {
		OpenCount(Cell::new(0))
	}

	fn increment(&self) {
		self.0.set(
			(|| -> Result<u32, std::io::Error> {
				let mut count_file = app_data!().user_data_dir().to_owned();
				fs::create_dir_all(&count_file)?;

				count_file.push(".open_count");

				let mut f = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(count_file)?;

				let count = f.read_u32::<LittleEndian>().unwrap_or(0) + 1;

				f.seek(SeekFrom::Start(0))?;
				f.write_u32::<LittleEndian>(count)?;

				Ok(count)
			})()
			.unwrap_or(0),
		);
	}
}
impl serde::Serialize for OpenCount {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		serializer.serialize_u32(self.0.get())
	}
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WorkshopUpdateMode {
	Description,
	Package,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChangelogMode {
	#[default]
	Blank,
	Template,
	LastEntered,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ChangelogDefaults {
	pub mode: ChangelogMode,
	pub template: String,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct ChangelogSettings {
	pub defaults: ChangelogDefaults,
	pub addons: HashMap<String, ChangelogDefaults>,
	pub drafts: HashMap<String, String>,
}

#[derive(Serialize)]
pub struct ChangelogEditorState {
	key: String,
	defaults: ChangelogDefaults,
	addon: Option<ChangelogDefaults>,
	mode: ChangelogMode,
	text: String,
}

impl ChangelogSettings {
	fn preferences(&self, key: &str) -> &ChangelogDefaults {
		self.addons.get(key).unwrap_or(&self.defaults)
	}

	fn editor(&self, key: String) -> ChangelogEditorState {
		let preferences = self.preferences(&key);
		let text = match preferences.mode {
			ChangelogMode::Blank => String::new(),
			ChangelogMode::Template => preferences.template.clone(),
			ChangelogMode::LastEntered => self.drafts.get(&key).cloned().unwrap_or_default(),
		};
		ChangelogEditorState {
			addon: self.addons.get(&key).cloned(),
			mode: preferences.mode.clone(),
			defaults: self.defaults.clone(),
			key,
			text,
		}
	}

	fn remember(&mut self, key: String, text: String) -> bool {
		if self.preferences(&key).mode != ChangelogMode::LastEntered {
			return false;
		}
		if self.drafts.get(&key) == Some(&text) {
			return false;
		}
		self.drafts.insert(key, text);
		true
	}

	pub fn published(&mut self, source_key: &str, id: PublishedFileId) {
		let key = format!("workshop:{}", id.0);
		if let Some(preferences) = self.addons.get(source_key).cloned() {
			self.addons.insert(key.clone(), preferences);
		}
		if let Some(draft) = self.drafts.get(source_key).cloned() {
			self.drafts.insert(key, draft);
		}
	}
}

pub fn changelog_key(addon_id: Option<PublishedFileId>, content_path: Option<PathBuf>) -> Result<String, String> {
	if let Some(id) = addon_id {
		if id.0 == 0 {
			return Err("Invalid Workshop addon ID".into());
		}
		return Ok(format!("workshop:{}", id.0));
	}
	if let Some(path) = content_path {
		let path = dunce::canonicalize(&path).map_err(|error| format!("Failed to identify addon folder {}: {}", path.display(), error))?;
		if !path.is_dir() {
			return Err(format!("Addon path is not a folder: {}", path.display()));
		}
		return Ok(format!("path:{}", path.to_string_lossy()));
	}
	Ok("new".into())
}

fn validate_changelog_key(key: &str) -> Result<(), String> {
	if key == "new"
		|| key.strip_prefix("workshop:").is_some_and(|id| id.parse::<u64>().is_ok_and(|id| id > 0))
		|| key
			.strip_prefix("path:")
			.is_some_and(|path| !path.contains('\0') && std::path::Path::new(path).is_absolute())
	{
		Ok(())
	} else {
		Err("Invalid changelog addon key".into())
	}
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkshopSort {
	UpdatedDesc,
	UpdatedAsc,
	SubscribersDesc,
	SubscribersAsc,
	CreatedDesc,
	CreatedAsc,
	TitleAsc,
	TitleDesc,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
	pub temp: Option<PathBuf>,
	pub gmod: Option<PathBuf>,
	pub user_data: Option<PathBuf>,
	pub downloads: Option<PathBuf>,

	pub sounds: bool,

	pub window_size: (f64, f64),
	pub window_maximized: bool,

	pub extract_destination: ExtractDestination,
	pub destinations: Vec<PathBuf>,
	pub create_folder_on_extract: bool,
	pub open_folder_after_extract: bool,

	pub ignore_globs: Vec<String>,

	pub my_workshop_local_paths: HashMap<PublishedFileId, PathBuf>,
	pub my_workshop_visibility: Vec<WorkshopVisibility>,
	pub my_workshop_sort: WorkshopSort,
	pub upscale_addon_icon: bool,
	pub workshop_update_mode: WorkshopUpdateMode,
	pub open_workshop_after_publish: bool,
	pub changelogs: ChangelogSettings,

	pub language: Option<String>,

	pub extract_overwrite_mode: ExtractionOverwriteMode,

	pub color_neutral: u32,
	pub color_error: u32,
	pub color_success: u32,
}
impl Default for Settings {
	fn default() -> Self {
		Self {
			temp: None,
			gmod: None,
			user_data: None,
			downloads: None,

			extract_destination: ExtractDestination::default(),
			sounds: true,

			window_size: (800., 600.),
			window_maximized: false,

			destinations: Vec::new(),
			create_folder_on_extract: true,
			open_folder_after_extract: true,

			ignore_globs: Vec::new(),
			my_workshop_local_paths: HashMap::new(),
			my_workshop_visibility: vec![
				WorkshopVisibility::Public,
				WorkshopVisibility::FriendsOnly,
				WorkshopVisibility::Private,
				WorkshopVisibility::Unlisted,
			],
			my_workshop_sort: WorkshopSort::UpdatedDesc,
			upscale_addon_icon: true,
			workshop_update_mode: WorkshopUpdateMode::Description,
			open_workshop_after_publish: true,
			changelogs: ChangelogSettings::default(),

			language: None,

			extract_overwrite_mode: ExtractionOverwriteMode::default(),

			color_neutral: 28103,
			color_error: 11010048,
			color_success: 3188321,
		}
	}
}
impl Settings {
	pub fn init() -> Settings {
		println!("Initializing Settings...");

		lazy_static::initialize(&HAD_SETTINGS_FILE);

		match Settings::load(&APP_SETTINGS_PATH, false) {
			Ok(settings) => settings,
			Err(error) if error.downcast_ref::<std::io::Error>().is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => Settings::default(),
			Err(error) => {
				*SETTINGS_RECOVERY.write() = Some(format!("{}: {}", APP_SETTINGS_PATH.display(), error));
				Settings::default()
			}
		}
	}

	fn load(path: &std::path::Path, sanitize: bool) -> Result<Settings, anyhow::Error> {
		let contents = fs::read_to_string(path)?;
		let mut settings: Settings = serde_json::de::from_str(&contents)?;
		if sanitize {
			settings.sanitize();
		}
		Ok(settings)
	}

	fn save(&self) -> Result<(), anyhow::Error> {
		if SETTINGS_RECOVERY.read().is_some() { anyhow::bail!("ERR_SETTINGS_RECOVERY_REQUIRED"); }
		self.save_to(&APP_SETTINGS_PATH)
	}

	fn save_to(&self, path: &Path) -> Result<(), anyhow::Error> {
		let parent = path.parent().ok_or_else(|| anyhow::anyhow!("Settings path has no parent"))?;
		fs::create_dir_all(parent)?;
		let bytes = serde_json::to_vec(self)?;
		match fs::read(path) {
			Ok(previous) => {
				if previous == bytes { return Ok(()); }
				// Never replace the known-good backup with a damaged file.
				if serde_json::from_slice::<Settings>(&previous).is_ok() {
					atomic_settings_write(&path.with_extension("json.bak"), &previous)?;
				}
			}
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
			Err(error) => return Err(error.into()),
		}
		atomic_settings_write(path, &bytes)?;
		Ok(())
	}

	pub fn sanitize(&mut self) {
		self.destinations.retain(|dir| dir.is_absolute() && dir.is_dir());
		self.my_workshop_local_paths.retain(|_, dir| dir.is_absolute() && dir.is_dir());

		match &self.extract_destination {
			ExtractDestination::Directory(path) => {
				if self.create_folder_on_extract || !path.is_dir() {
					self.extract_destination = ExtractDestination::NamedDirectory(path.to_owned());
				}
			}
			ExtractDestination::NamedDirectory(path) => {
				if !self.create_folder_on_extract || !path.is_dir() {
					self.extract_destination = ExtractDestination::Directory(path.to_owned());
				}
			}
			ExtractDestination::Downloads => {
				if app_data!().downloads_dir().is_none() {
					self.extract_destination = ExtractDestination::default();
				}
			}
			ExtractDestination::Addons if app_data!().gmod_dir().is_none() => {
				self.extract_destination = ExtractDestination::default();
			}
			_ => {}
		}

		self.destinations.truncate(20);
	}
}

#[derive(Debug, Serialize)]
pub struct AppData {
	pub settings: RwLock<Settings>,
	pub version: &'static str,
	pub open_count: OpenCount,

	#[serde(serialize_with = "serde_temp_dir")]
	temp_dir: PathBuf,
	#[serde(serialize_with = "serde_gmod_dir")]
	gmod_dir: Option<PathBuf>,
	#[serde(serialize_with = "serde_user_data_dir")]
	user_data_dir: PathBuf,
	#[serde(serialize_with = "serde_downloads_dir")]
	downloads_dir: Option<PathBuf>,
}
impl AppData {
	pub fn init() -> Self {
		let settings = Settings::init();
		Self {
			settings: RwLock::new(settings),
			version: env!("CARGO_PKG_VERSION"),
			open_count: OpenCount::init(),

			// Placeholders
			temp_dir: PathBuf::new(),
			user_data_dir: PathBuf::new(),
			gmod_dir: None,
			downloads_dir: None,
		}
	}

	pub fn send(&'static self) {
		webview_emit!("UpdateAppData", self);
	}

	pub fn gmod_dir(&self) -> Option<PathBuf> {
		println!("Locating Garry's Mod...");

		if let Some(ref gmod) = self.settings.read().gmod {
			if gmod.is_dir() {
				println!("Using user-defined path");
				return Some(gmod.to_owned());
			}
		}

		if !steam!().connected() {
			println!("Steam is not connected, parsing Steam library folders...");
			match steamlocate::locate().and_then(|steam_dir| steam_dir.find_app(GMOD_APP_ID.0)) {
				Ok(Some((app, library))) => {
					println!("Located!");
					return Some(library.resolve_app_dir(&app));
				}
				Ok(None) => println!("Garry's Mod was not found in Steam library folders."),
				Err(error) => eprintln!("Failed to locate Garry's Mod in Steam library folders: {}", error),
			}
			println!("Waiting for Steam...");
			for i in 0..3_u8 {
				sleep!(1);
				if steam!().connected() {
					println!("Steam connected!");
					break;
				} else if i == 2 {
					println!("Gave up.");
					return None;
				}
			}
		}

		println!("Getting Garry's Mod location from Steamworks...");
		let gmod: PathBuf = steam!().client().apps().app_install_dir(GMOD_APP_ID).into();
		if gmod.is_dir() {
			println!("Located!");
			Some(gmod)
		} else {
			println!("Failed.");
			None
		}
	}

	pub fn temp_dir(&self) -> RwLockCow<'_, PathBuf> {
		let lock = self.settings.read();
		if let Some(ref temp) = lock.temp {
			if temp.is_dir() {
				return RwLockCow::Locked(RwLockReadGuard::map(lock, |s| s.temp.as_ref().unwrap()));
			}
		}

		RwLockCow::Borrowed(&*TEMP_DIR)
	}

	pub fn user_data_dir(&self) -> RwLockCow<'_, PathBuf> {
		let lock = self.settings.read();
		if let Some(ref user_data) = lock.user_data {
			if user_data.is_dir() {
				return RwLockCow::Locked(RwLockReadGuard::map(lock, |s| s.user_data.as_ref().unwrap()));
			}
		}

		RwLockCow::Borrowed(&*USER_DATA_DIR)
	}

	pub fn downloads_dir(&self) -> RwLockCow<'_, Option<PathBuf>> {
		let lock = self.settings.read();
		if let Some(ref downloads) = lock.downloads {
			if downloads.is_dir() {
				return RwLockCow::Locked(RwLockReadGuard::map(lock, |s| &s.downloads));
			}
		}

		RwLockCow::Borrowed(&*DOWNLOADS_DIR)
	}
}

#[cfg(target_os = "windows")]
const PATH_SEPARATOR: char = '\\';
#[cfg(not(target_os = "windows"))]
const PATH_SEPARATOR: char = '/';

pub struct Plugin;
impl<R: tauri::Runtime> tauri::plugin::Plugin<R> for Plugin {
	fn initialization_script(&self) -> Option<String> {
		let mut sanitized = app_data!().settings.read().clone();
		sanitized.sanitize();
		*app_data!().settings.write() = sanitized;

		let mut default_ignore: Vec<String> = crate::gma::DEFAULT_IGNORE.iter().map(|x| x.to_string()).collect();
		default_ignore.sort();

		app_data!().open_count.increment();

		Some(
			include_str!("../../app/plugins/AppData.js")
				.replacen(
					"{$_APP_DATA_$}",
					&crate::escape_single_quoted_json(serde_json::ser::to_string(&*crate::APP_DATA).unwrap()),
					1,
				)
				.replacen(
					"{$_WS_DEAD_$}",
					&crate::escape_single_quoted_json(
						serde_json::ser::to_string(&crate::WorkshopItem::from(steamworks::PublishedFileId(0))).unwrap(),
					),
					1,
				)
				.replacen(
					"{$_DEFAULT_IGNORE_GLOBS_$}",
					&crate::escape_single_quoted_json(serde_json::ser::to_string(&default_ignore).unwrap()),
					1,
				)
				.replacen("{$_PATH_SEPARATOR_$}", &serde_json::ser::to_string(&PATH_SEPARATOR).unwrap(), 1),
		)
	}

	fn name(&self) -> &'static str {
		"AppData"
	}
}

fn atomic_settings_write(path: &Path, bytes: &[u8]) -> Result<(), anyhow::Error> {
	let parent = path.parent().ok_or_else(|| anyhow::anyhow!("Settings path has no parent"))?;
	let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
	temporary.write_all(bytes)?;
	temporary.as_file().sync_all()?;
	temporary.persist(path).map_err(|error| error.error)?;
	#[cfg(unix)]
	File::open(parent)?.sync_all()?;
	Ok(())
}

pub fn change_settings<T>(change: impl FnOnce(&mut Settings) -> Result<T, String>) -> Result<T, String> {
	let _commit = SETTINGS_COMMIT.lock();
	let mut next = app_data!().settings.read().clone();
	let result = change(&mut next)?;
	next.save().map_err(|error| format!("ERR_SETTINGS_SAVE:{}", error))?;
	*app_data!().settings.write() = next;
	Ok(result)
}

fn patched_settings(settings: &Settings, patch: serde_json::Map<String, serde_json::Value>) -> Result<Settings, String> {
	let mut value = serde_json::to_value(settings).map_err(|error| error.to_string())?;
	let fields = value.as_object_mut().unwrap();
	for (key, value) in patch {
		if !fields.contains_key(&key) || matches!(key.as_str(), "window_size" | "window_maximized" | "changelogs" | "my_workshop_local_paths") {
			return Err(format!("ERR_SETTINGS_FIELD:{}", key));
		}
		fields.insert(key, value);
	}
	serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_settings(patch: serde_json::Map<String, serde_json::Value>) -> Result<bool, String> {
	let rediscover_addons = change_settings(|settings| {
		let old_gmod = settings.gmod.clone();
		*settings = patched_settings(settings, patch)?;
		let source_paths = settings.my_workshop_local_paths.clone();
		settings.sanitize();
		settings.my_workshop_local_paths = source_paths;
		Ok(old_gmod != settings.gmod)
	})?;
	if rediscover_addons {
		game_addons!().refresh();
		webview_emit!("InstalledAddonsRefreshed");
	}
	app_data!().send();
	Ok(true)
}

#[tauri::command]
pub fn settings_recovery() -> Option<(String, bool)> {
	SETTINGS_RECOVERY.read().clone().map(|error| (error, Settings::load(&APP_SETTINGS_PATH.with_extension("json.bak"), false).is_ok()))
}

#[tauri::command]
pub fn recover_settings(use_backup: bool) -> Result<(), String> {
	let _commit = SETTINGS_COMMIT.lock();
	if SETTINGS_RECOVERY.read().is_none() { return Ok(()); }
	let result = (|| -> Result<Settings, anyhow::Error> {
		let settings = if use_backup { Settings::load(&APP_SETTINGS_PATH.with_extension("json.bak"), false)? } else { Settings::default() };
		let parent = APP_SETTINGS_PATH.parent().unwrap();
		let mut damaged = tempfile::Builder::new().prefix("settings-damaged-").suffix(".json").tempfile_in(parent)?;
		std::io::copy(&mut File::open(&*APP_SETTINGS_PATH)?, &mut damaged)?;
		damaged.as_file().sync_all()?;
		damaged.keep().map_err(|error| error.error)?;
		settings.save_to(&APP_SETTINGS_PATH)?;
		Ok(settings)
	})();
	let settings = result.map_err(|error| format!("ERR_SETTINGS_SAVE:{}", error))?;
	*app_data!().settings.write() = settings;
	*SETTINGS_RECOVERY.write() = None;
	Ok(())
}

#[tauri::command]
pub fn get_changelog(addon_id: Option<PublishedFileId>, content_path: Option<PathBuf>) -> Result<ChangelogEditorState, String> {
	let key = changelog_key(addon_id, content_path)?;
	Ok(app_data!().settings.read().changelogs.editor(key))
}

#[tauri::command]
pub fn save_changelog_defaults(key: String, global: bool, preferences: Option<ChangelogDefaults>) -> Result<ChangelogEditorState, String> {
	validate_changelog_key(&key)?;
	change_settings(|next| {
		if global {
			next.changelogs.defaults = preferences.ok_or("Global changelog defaults are required")?;
		} else if key == "new" {
			return Err("Select an addon folder before setting addon defaults".into());
		} else if let Some(preferences) = preferences {
			next.changelogs.addons.insert(key.clone(), preferences);
		} else {
			next.changelogs.addons.remove(&key);
		}
		Ok(next.changelogs.editor(key))
	})
}

#[tauri::command]
pub fn remember_changelog(key: String, text: String) -> Result<(), String> {
	validate_changelog_key(&key)?;
	change_settings(|next| { next.changelogs.remember(key, text); Ok(()) })
}

/// Whether there are settings from an older gmpublisher installation to import.
/// Only true on a first launch, so the user is never asked twice.
#[tauri::command]
pub fn legacy_settings_pending() -> bool {
	!MIGRATION_RESOLVED.load(Ordering::Relaxed) && !*HAD_SETTINGS_FILE && LEGACY_APP_SETTINGS_PATH.is_file()
}

/// Imports the settings left behind by gmpublisher's config directory.
#[tauri::command]
pub fn migrate_legacy_settings() -> Result<(), String> {
	let settings = Settings::load(&LEGACY_APP_SETTINGS_PATH, true)
		.map_err(|error| format!("Failed to read gmpublisher settings: {}", error))?;
	let rediscover_addons = change_settings(|current| {
		let changed = current.gmod != settings.gmod;
		*current = settings;
		Ok(changed)
	})?;
	MIGRATION_RESOLVED.store(true, Ordering::Relaxed);

	if rediscover_addons {
		game_addons!().refresh();
		webview_emit!("InstalledAddonsRefreshed");
	}

	webview_emit!("UpdateAppData", &*crate::APP_DATA);

	Ok(())
}

/// Keeps the settings nwmpublisher is currently using, writing them out so that
/// the user isn't asked to migrate again on the next launch.
#[tauri::command]
pub fn dismiss_legacy_settings() -> Result<(), String> {
	change_settings(|_| Ok(()))?;
	MIGRATION_RESOLVED.store(true, Ordering::Relaxed);
	Ok(())
}

#[tauri::command]
pub fn validate_gmod(mut path: PathBuf) -> bool {
	path.push("GarrysMod");
	path.push("addons");
	path.is_absolute() && path.is_dir()
}

#[tauri::command]
pub fn window_resized(window: tauri::Window, width: f64, height: f64) -> Result<(), String> {
	let size = window.outer_size().and_then(|size| Ok(size.to_logical(window.scale_factor()?)))
		.map(|size| (size.width, size.height)).map_err(|error| error.to_string())?;
	if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. { return Err("ERR_SETTINGS_WINDOW_SIZE".into()); }
	let maximized = window.is_maximized().map_err(|error| error.to_string())?;
	change_settings(|settings| {
		if !maximized { settings.window_size = size; }
		settings.window_maximized = maximized;
		Ok(())
	})
}

fn serde_gmod_dir<S>(_: &Option<PathBuf>, serializer: S) -> Result<S::Ok, S::Error>
where
	S: serde::Serializer,
{
	app_data!().gmod_dir().serialize(serializer)
}

fn serde_temp_dir<S>(_: &PathBuf, serializer: S) -> Result<S::Ok, S::Error>
where
	S: serde::Serializer,
{
	app_data!().temp_dir().serialize(serializer)
}

fn serde_user_data_dir<S>(_: &PathBuf, serializer: S) -> Result<S::Ok, S::Error>
where
	S: serde::Serializer,
{
	app_data!().user_data_dir().serialize(serializer)
}

fn serde_downloads_dir<S>(_: &Option<PathBuf>, serializer: S) -> Result<S::Ok, S::Error>
where
	S: serde::Serializer,
{
	app_data!().downloads_dir().serialize(serializer)
}

pub fn write_tauri_settings() -> Option<()> {
	use serde_json::Value as JsonValue;
	use std::io::{BufReader, BufWriter};

	let mut settings_path = dirs_next::config_dir()?;
	settings_path.push("nwmpublisher");

	fs::create_dir_all(&settings_path).ok()?;

	settings_path.push(".tauri-settings.json");

	if settings_path.exists() {
		let stored = {
			let mut stored: HashMap<String, JsonValue> = serde_json::from_reader(BufReader::new(File::open(&settings_path).ok()?)).ok()?;
			stored.insert("allow_notification".to_string(), JsonValue::Bool(true));
			stored
		};
		serde_json::to_writer(BufWriter::new(File::create(settings_path).ok()?), &stored).ok()?;
	} else {
		fs::write(settings_path, r#"{"allow_notification":true}"#).ok()?;
	}

	Some(())
}

#[cfg(test)]
mod tests {
	#[test]
	fn atomic_saves_keep_a_valid_backup_and_preserve_the_live_file_on_failure() {
		let root = tempfile::tempdir().unwrap();
		let path = root.path().join("settings.json");
		let mut settings = super::Settings::default();
		settings.save_to(&path).unwrap();
		let original = std::fs::read(&path).unwrap();
		settings.sounds = false;
		settings.save_to(&path).unwrap();
		assert_eq!(std::fs::read(path.with_extension("json.bak")).unwrap(), original);
		let current = std::fs::read(&path).unwrap();
		std::fs::remove_file(path.with_extension("json.bak")).unwrap();
		std::fs::create_dir(path.with_extension("json.bak")).unwrap();
		settings.sounds = true;
		assert!(settings.save_to(&path).is_err());
		assert_eq!(std::fs::read(&path).unwrap(), current);
	}

	#[test]
	fn field_patches_preserve_newer_window_state_paths_and_drafts() {
		let mut settings = super::Settings::default();
		settings.window_size = (1280., 720.);
		settings.my_workshop_local_paths.insert(steamworks::PublishedFileId(7), std::path::PathBuf::from("source"));
		settings.changelogs.drafts.insert("workshop:7".into(), "draft".into());
		let patch = serde_json::json!({ "sounds": false }).as_object().unwrap().clone();
		let result = super::patched_settings(&settings, patch).unwrap();
		assert!(!result.sounds);
		assert_eq!(result.window_size, settings.window_size);
		assert_eq!(result.my_workshop_local_paths, settings.my_workshop_local_paths);
		assert_eq!(result.changelogs.drafts, settings.changelogs.drafts);
		let patch = serde_json::json!({ "my_workshop_local_paths": {} }).as_object().unwrap().clone();
		assert!(super::patched_settings(&settings, patch).is_err());
	}
	use super::{
		changelog_key, validate_changelog_key, ChangelogDefaults, ChangelogMode, ChangelogSettings, Settings, WorkshopSort, WorkshopUpdateMode,
		WorkshopVisibility,
	};
	use std::fs::{self, File};
	use steamworks::PublishedFileId;

	#[test]
	fn workshop_browsing_defaults_preserve_existing_settings() {
		let settings: Settings = serde_json::from_str(r#"{"sounds":false,"ignore_globs":["*.bak"]}"#).unwrap();
		assert_eq!(settings.my_workshop_sort, WorkshopSort::UpdatedDesc);
		assert_eq!(
			settings.my_workshop_visibility,
			vec![
				WorkshopVisibility::Public,
				WorkshopVisibility::FriendsOnly,
				WorkshopVisibility::Private,
				WorkshopVisibility::Unlisted,
			]
		);
		assert!(!settings.sounds);
		assert_eq!(settings.ignore_globs, vec!["*.bak"]);
	}

	#[test]
	fn workshop_browsing_preferences_round_trip_including_no_visibility() {
		let visibility_options = ["public", "friends_only", "private", "unlisted"];
		for sort in [
			"updated_desc",
			"updated_asc",
			"subscribers_desc",
			"subscribers_asc",
			"created_desc",
			"created_asc",
			"title_asc",
			"title_desc",
		] {
			for mask in 0..16 {
				let visibility: Vec<_> = visibility_options
					.iter()
					.enumerate()
					.filter_map(|(index, value)| (mask & (1 << index) != 0).then_some(*value))
					.collect();
				let settings: Settings = serde_json::from_value(serde_json::json!({
					"my_workshop_visibility": visibility,
					"my_workshop_sort": sort,
					"sounds": false,
				}))
				.unwrap();
				let saved = serde_json::to_value(&settings).unwrap();
				assert_eq!(saved["my_workshop_visibility"], serde_json::json!(visibility));
				assert_eq!(saved["my_workshop_sort"], sort);
				let loaded: Settings = serde_json::from_value(saved).unwrap();
				assert_eq!(loaded.my_workshop_visibility, settings.my_workshop_visibility);
				assert_eq!(loaded.my_workshop_sort, settings.my_workshop_sort);
				assert!(!loaded.sounds);
			}
		}
	}

	#[test]
	fn workshop_browsing_preferences_reject_invalid_values() {
		assert!(serde_json::from_value::<Settings>(serde_json::json!({"my_workshop_sort": "invalid"})).is_err());
		assert!(serde_json::from_value::<Settings>(serde_json::json!({"my_workshop_visibility": ["invalid"]})).is_err());
	}

	#[test]
	fn automatic_open_preferences_preserve_legacy_settings() {
		let settings: Settings = serde_json::from_str(r#"{"sounds":false,"ignore_globs":["*.bak"],"create_folder_on_extract":false}"#).unwrap();
		assert!(settings.open_workshop_after_publish);
		assert!(settings.open_folder_after_extract);
		assert!(!settings.sounds);
		assert!(!settings.create_folder_on_extract);
		assert_eq!(settings.ignore_globs, vec!["*.bak"]);
	}

	#[test]
	fn automatic_open_preferences_round_trip_independently() {
		for publish in [false, true] {
			for extract in [false, true] {
				let settings = Settings {
					open_workshop_after_publish: publish,
					open_folder_after_extract: extract,
					..Settings::default()
				};
				let saved = serde_json::to_value(&settings).unwrap();
				assert_eq!(saved["open_workshop_after_publish"], publish);
				assert_eq!(saved["open_folder_after_extract"], extract);
				let loaded: Settings = serde_json::from_value(saved).unwrap();
				assert_eq!(loaded.open_workshop_after_publish, publish);
				assert_eq!(loaded.open_folder_after_extract, extract);
			}
		}
	}

	#[test]
	fn missing_automatic_open_preference_does_not_reset_the_other() {
		let settings: Settings = serde_json::from_str(r#"{"open_workshop_after_publish":false}"#).unwrap();
		assert!(!settings.open_workshop_after_publish);
		assert!(settings.open_folder_after_extract);
		let settings: Settings = serde_json::from_str(r#"{"open_folder_after_extract":false}"#).unwrap();
		assert!(settings.open_workshop_after_publish);
		assert!(!settings.open_folder_after_extract);
	}

	#[test]
	fn workshop_update_mode_defaults_without_resetting_existing_settings() {
		let settings: Settings = serde_json::from_str(r#"{"sounds":false,"ignore_globs":["*.bak"]}"#).unwrap();
		assert_eq!(settings.workshop_update_mode, WorkshopUpdateMode::Description);
		assert!(!settings.sounds);
		assert_eq!(settings.ignore_globs, vec!["*.bak"]);
	}

	#[test]
	fn workshop_update_mode_is_preserved_in_settings() {
		for mode in ["description", "package"] {
			let settings: Settings = serde_json::from_value(serde_json::json!({"workshop_update_mode": mode})).unwrap();
			let saved = serde_json::to_value(&settings).unwrap();
			assert_eq!(saved["workshop_update_mode"], mode);
			let loaded: Settings = serde_json::from_value(saved).unwrap();
			assert_eq!(loaded.workshop_update_mode, settings.workshop_update_mode);
		}
		assert!(serde_json::from_value::<Settings>(serde_json::json!({"workshop_update_mode": "invalid"})).is_err());
	}

	#[test]
	fn changelog_defaults_preserve_existing_settings() {
		let settings: Settings = serde_json::from_str(r#"{"sounds":false,"ignore_globs":["*.bak"]}"#).unwrap();
		assert!(!settings.sounds);
		assert_eq!(settings.ignore_globs, vec!["*.bak"]);
		assert_eq!(settings.changelogs.defaults.mode, ChangelogMode::Blank);
		assert!(settings.changelogs.editor("workshop:1".into()).text.is_empty());
		assert!(settings.changelogs.addons.is_empty());
		assert!(settings.changelogs.drafts.is_empty());
	}

	#[test]
	fn changelog_addon_modes_override_global_defaults() {
		let mut settings = ChangelogSettings {
			defaults: ChangelogDefaults {
				mode: ChangelogMode::Template,
				template: "[b]Global[/b]\n".into(),
			},
			..ChangelogSettings::default()
		};
		let key = "workshop:1".to_owned();
		assert_eq!(settings.editor(key.clone()).text, "[b]Global[/b]\n");
		settings.addons.insert(key.clone(), ChangelogDefaults::default());
		assert!(settings.editor(key.clone()).text.is_empty());
		settings.addons.insert(
			key.clone(),
			ChangelogDefaults {
				mode: ChangelogMode::Template,
				template: "Addon template".into(),
			},
		);
		assert_eq!(settings.editor(key.clone()).text, "Addon template");
		assert_eq!(settings.editor("workshop:2".into()).text, "[b]Global[/b]\n");
		settings.addons.remove(&key);
		assert_eq!(settings.editor(key).text, "[b]Global[/b]\n");
	}

	#[test]
	fn changelog_drafts_are_opt_in_and_isolated_including_empty_text() {
		let mut settings = ChangelogSettings::default();
		let key = "workshop:1".to_owned();
		assert!(!settings.remember(key.clone(), "Not remembered".into()));
		settings.defaults.mode = ChangelogMode::LastEntered;
		assert!(settings.editor(key.clone()).text.is_empty());
		assert!(settings.remember(key.clone(), "[b]Unpublished draft[/b]\n".into()));
		assert_eq!(settings.editor(key.clone()).text, "[b]Unpublished draft[/b]\n");
		assert!(settings.editor("workshop:2".into()).text.is_empty());
		assert!(settings.remember(key.clone(), String::new()));
		assert!(settings.editor(key.clone()).text.is_empty());
		assert!(!settings.remember(key, String::new()));
	}

	#[test]
	fn changelog_mode_changes_keep_templates_separate_from_drafts() {
		let mut settings = ChangelogSettings {
			defaults: ChangelogDefaults {
				mode: ChangelogMode::LastEntered,
				template: "Template".into(),
			},
			..ChangelogSettings::default()
		};
		let key = "workshop:1".to_owned();
		settings.remember(key.clone(), "Draft".into());
		settings.defaults.mode = ChangelogMode::Template;
		assert_eq!(settings.editor(key.clone()).text, "Template");
		assert!(!settings.remember(key.clone(), "Template edit".into()));
		settings.defaults.mode = ChangelogMode::LastEntered;
		assert_eq!(settings.editor(key).text, "Draft");
	}

	#[test]
	fn changelog_settings_survive_reload_and_first_publish() {
		let folder = tempfile::tempdir().unwrap();
		let key = changelog_key(None, Some(folder.path().to_owned())).unwrap();
		let mut settings = Settings::default();
		settings.changelogs.addons.insert(
			key.clone(),
			ChangelogDefaults {
				mode: ChangelogMode::LastEntered,
				template: "[h1]Changes[/h1]\n".into(),
			},
		);
		settings.changelogs.remember(key.clone(), "Draft with Unicode: café\n".into());
		settings.changelogs.published(&key, PublishedFileId(123));
		let file = folder.path().join("settings.json");
		serde_json::to_writer(File::create(&file).unwrap(), &settings).unwrap();
		let loaded = Settings::load(&file, false).unwrap();
		for key in [key, "workshop:123".into()] {
			assert_eq!(loaded.changelogs.editor(key.clone()).text, "Draft with Unicode: café\n");
			assert_eq!(loaded.changelogs.preferences(&key).template, "[h1]Changes[/h1]\n");
		}
		assert!(loaded.changelogs.editor("workshop:456".into()).text.is_empty());
	}

	#[test]
	fn changelog_first_publish_keeps_global_inheritance() {
		let mut settings = ChangelogSettings::default();
		settings.defaults.mode = ChangelogMode::LastEntered;
		settings.remember("source".into(), "Draft".into());
		settings.published("source", PublishedFileId(123));
		assert!(settings.addons.is_empty());
		assert_eq!(settings.editor("workshop:123".into()).text, "Draft");
		settings.defaults.mode = ChangelogMode::Template;
		settings.defaults.template = "New default".into();
		assert_eq!(settings.editor("workshop:123".into()).text, "New default");
	}

	#[test]
	fn changelog_rejects_invalid_modes_keys_and_paths() {
		assert!(serde_json::from_value::<Settings>(serde_json::json!({"changelogs": {"defaults": {"mode": "invalid"}}})).is_err());
		for key in ["", "workshop:0", "workshop:no", "path:relative"] {
			assert!(validate_changelog_key(key).is_err());
		}
		assert!(validate_changelog_key("workshop:123").is_ok());
		assert_eq!(changelog_key(None, None).unwrap(), "new");
		assert!(changelog_key(Some(PublishedFileId(0)), None).is_err());
		let folder = tempfile::tempdir().unwrap();
		assert!(changelog_key(None, Some(folder.path().join("missing"))).is_err());
		let file = folder.path().join("file");
		fs::write(&file, "").unwrap();
		assert!(changelog_key(None, Some(file)).is_err());
		let key = changelog_key(None, Some(folder.path().join("."))).unwrap();
		assert_eq!(key, changelog_key(None, Some(folder.path().to_owned())).unwrap());
		assert!(validate_changelog_key(&key).is_ok());
	}
}
