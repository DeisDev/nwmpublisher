use crate::{
	gma::{manifest::ContentManifest, GMAEntry, GMAError, GMAFile, GMAFilePointers, GMAMetadata},
	Transaction, GMOD_APP_ID,
};
use image::{DynamicImage, GenericImageView, ImageError, ImageFormat};
use parking_lot::Mutex;
use std::{
	fs::File,
	io::{BufReader, BufWriter, Write},
	path::{Path, PathBuf},
	sync::Arc,
};
use steamworks::{PublishedFileId, SteamError};

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
	Journal(String),
	Unknown,
	Cancelled,
	NoEntries,
	InvalidContentPath,
	MultipleGMAs,
	IconTooLarge,
	IconTooSmall,
	IconInvalidFormat,
	DescriptionTooLong,
	DescriptionContainsNul,
	IOError(#[source] crate::IoError),
	SteamError(#[source] SteamError),
	ImageError {
		operation: &'static str,
		path: PathBuf,
		#[source]
		source: ImageError,
	},
}
impl std::fmt::Display for PublishError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			PublishError::Journal(error) => write!(f, "ERR_PUBLISH_JOURNAL:{}", error),
			PublishError::Unknown => write!(f, "ERR_PUBLISH_OUTCOME_UNKNOWN"),
			PublishError::Cancelled => write!(f, "ERR_CANCELLED"),
			PublishError::NoEntries => write!(f, "ERR_NO_ENTRIES"),
			PublishError::InvalidContentPath => write!(f, "ERR_INVALID_CONTENT_PATH"),
			PublishError::MultipleGMAs => write!(f, "ERR_MULTIPLE_GMAS"),
			PublishError::IconTooLarge => write!(f, "ERR_ICON_TOO_LARGE"),
			PublishError::IconTooSmall => write!(f, "ERR_ICON_TOO_SMALL"),
			PublishError::IconInvalidFormat => write!(f, "ERR_ICON_INVALID_FORMAT"),
			PublishError::DescriptionTooLong => write!(f, "ERR_DESCRIPTION_TOO_LONG"),
			PublishError::DescriptionContainsNul => write!(f, "ERR_DESCRIPTION_CONTAINS_NUL"),
			PublishError::IOError(error) => write!(f, "ERR_IO_ERROR:{}", error),
			PublishError::SteamError(error) => write!(f, "ERR_STEAM_ERROR:{}", error),
			PublishError::ImageError { operation, path, source } => write!(f, "ERR_IMAGE_ERROR:{} \"{}\": {}", operation, path.display(), source),
		}
	}
}
impl serde::Serialize for PublishError {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		serializer.serialize_str(&self.to_string())
	}
}
impl From<SteamError> for PublishError {
	fn from(error: SteamError) -> PublishError {
		PublishError::SteamError(error)
	}
}
impl PublishError {
	pub fn io(operation: &'static str, path: &Path, error: std::io::Error) -> Self {
		Self::IOError(crate::IoError::new(operation, path, error))
	}

	pub fn image(operation: &'static str, path: &Path, error: ImageError) -> Self {
		match error {
			ImageError::IoError(error) => Self::io(operation, path, error),
			source => Self::ImageError {
				operation,
				path: path.to_owned(),
				source,
			},
		}
	}
}

use super::{Steam, publish_jobs::PublishJob};
pub struct ContentPath(PathBuf);
impl std::ops::Deref for ContentPath {
	type Target = PathBuf;
	fn deref(&self) -> &Self::Target {
		&self.0
	}
}
impl From<ContentPath> for PathBuf {
	fn from(val: ContentPath) -> Self {
		val.0
	}
}
impl ContentPath {
	pub fn new(path: PathBuf) -> Result<ContentPath, PublishError> {
		if !path
			.metadata()
			.map_err(|error| PublishError::io("read content metadata", &path, error))?
			.is_dir()
		{
			return Err(PublishError::InvalidContentPath);
		}

		let mut found_gma = false;
		for entry in path
			.read_dir()
			.map_err(|error| PublishError::io("read content directory", &path, error))?
		{
			let entry = entry.map_err(|error| PublishError::io("read directory entry", &path, error))?;
			let entry_path = entry.path();
			if !entry
				.file_type()
				.map_err(|error| PublishError::io("read content file type", &entry_path, error))?
				.is_file() || entry_path.extension().is_none_or(|extension| !extension.eq_ignore_ascii_case("gma"))
			{
				return Err(PublishError::InvalidContentPath);
			}

			if found_gma {
				return Err(PublishError::MultipleGMAs);
			}
			found_gma = true;
		}

		if found_gma {
			Ok(ContentPath(path))
		} else {
			Err(PublishError::NoEntries)
		}
	}
}

#[derive(Debug)]
struct StagingResult<T> { value: T, warnings: Vec<String> }

fn with_publish_staging<T>(parent: &Path, operation: impl FnOnce(&Path, &Path) -> Result<T, String>) -> Result<StagingResult<T>, String> {
	std::fs::create_dir_all(parent).map_err(|error| PublishError::io("create publishing parent directory", parent, error).to_string())?;
	let staging = tempfile::Builder::new()
		.prefix("nwmpublisher-publishing-")
		.tempdir_in(parent)
		.map_err(|error| PublishError::io("create publishing staging directory", parent, error).to_string())?;
	let root = staging.path().to_owned();
	let content = root.join("content");
	let result = std::fs::create_dir(&content)
		.map_err(|error| PublishError::io("create publishing content directory", &content, error).to_string())
		.and_then(|()| operation(&root, &content));

	if result.as_ref().is_err_and(|error| error == "ERR_PUBLISH_OUTCOME_UNKNOWN") {
		let _retained = staging.into_path();
		return Err("ERR_PUBLISH_OUTCOME_UNKNOWN".into());
	}
	let ownership = std::fs::read(root.join("operation-id")).ok();
	// Steam's completion callback must have returned before the operation releases these files.
	let cleanup = staging
		.close()
		.map_err(|error| PublishError::io("remove publishing staging directory", &root, error));
	if cleanup.is_err() {
		if let Some(ownership) = ownership {
			if let Err(error) = std::fs::write(root.join("operation-id"), ownership) { eprintln!("Restore staging ownership {}: {}", root.display(), error); }
		}
	}
	match (result, cleanup) {
		(Ok(value), Ok(())) => Ok(StagingResult { value, warnings: Vec::new() }),
		(Err(error), Ok(())) => Err(error),
		(Ok(value), Err(error)) => Ok(StagingResult { value, warnings: vec![error.to_string()] }),
		(Err(error), Err(cleanup)) => Err(format!("{}\nCleanup failed: {}", error, cleanup)),
	}
}

const WORKSHOP_ICON_MAX_SIZE: u64 = 1048576;
const WORKSHOP_ICON_MIN_SIZE: u64 = 16;

pub enum WorkshopIcon {
	Custom {
		image: DynamicImage,
		path: PathBuf,
		format: ImageFormat,
		width: u32,
		height: u32,
		upscale: bool,
	},
	Default,
}
impl WorkshopIcon {
	pub fn can_upscale(width: u32, height: u32, format: ImageFormat) -> bool {
		!matches!(format, ImageFormat::Gif) && ((width < 512 || height < 512) || (width != height))
	}
}
impl WorkshopIcon {
	pub fn into_path(self, directory: &Path) -> Result<PathBuf, PublishError> {
		match self {
			WorkshopIcon::Custom {
				path,
				image,
				width,
				height,
				upscale,
				format,
			} => {
				if upscale && WorkshopIcon::can_upscale(width, height, format) {
					let extension = match format {
						ImageFormat::Png => "png",
						ImageFormat::Jpeg => "jpg",
						_ => return Err(PublishError::IconInvalidFormat),
					};
					let output = directory.join(format!("nwmpublisher_upscaled_icon.{}", extension));
					let image = image.resize_exact(512, 512, image::imageops::FilterType::CatmullRom);
					write_icon(&image, &output, format)?;
					Ok(output)
				} else {
					Ok(path)
				}
			}
			WorkshopIcon::Default => {
				let path = directory.join("default_workshop_icon.png");
				let image = DynamicImage::ImageRgba8(super::default_icon::compose()?);
				write_icon(&image, &path, ImageFormat::Png)?;
				Ok(path)
			}
		}
	}
}

fn encode_icon<W: Write>(image: &DynamicImage, writer: &mut W, path: &Path, format: ImageFormat) -> Result<(), PublishError> {
	image
		.write_to(writer, format)
		.map_err(|error| PublishError::image("encode icon", path, error))?;
	writer.flush().map_err(|error| PublishError::io("flush icon", path, error))
}

fn write_icon(image: &DynamicImage, path: &Path, format: ImageFormat) -> Result<(), PublishError> {
	let file = File::create(path).map_err(|error| PublishError::io("create icon", path, error))?;
	encode_icon(image, &mut BufWriter::new(file), path, format)
}

impl WorkshopIcon {
	pub fn new<P: AsRef<Path>>(path: P, upscale: bool) -> Result<WorkshopIcon, PublishError> {
		let path = path.as_ref();

		let len = path
			.metadata()
			.map_err(|error| PublishError::io("read icon metadata", path, error))?
			.len();
		if len > WORKSHOP_ICON_MAX_SIZE {
			return Err(PublishError::IconTooLarge);
		} else if len < WORKSHOP_ICON_MIN_SIZE {
			return Err(PublishError::IconTooSmall);
		}

		let file_extension = path.extension().and_then(|x| x.to_str()).unwrap_or("jpg").to_ascii_lowercase();
		let image_format = match file_extension.as_str() {
			"png" => ImageFormat::Png,
			"gif" => ImageFormat::Gif,
			"jpeg" | "jpg" => ImageFormat::Jpeg,
			_ => return Err(PublishError::IconInvalidFormat),
		};

		let file = File::open(path).map_err(|error| PublishError::io("open icon", path, error))?;
		let image = image::load(BufReader::new(file), image_format).map_err(|error| PublishError::image("decode icon", path, error))?;
		Ok(WorkshopIcon::Custom {
			path: path.to_path_buf(),
			width: image.width(),
			height: image.height(),
			format: image_format,
			upscale,
			image,
		})
	}
}

// k_cchPublishedDocumentDescriptionMax is 8000, including the C string terminator.
const WORKSHOP_DESCRIPTION_MAX_BYTES: usize = 7999;

fn validate_description(description: Option<&str>) -> Result<(), PublishError> {
	if let Some(description) = description {
		if description.len() > WORKSHOP_DESCRIPTION_MAX_BYTES {
			return Err(PublishError::DescriptionTooLong);
		}
		if description.contains('\0') {
			return Err(PublishError::DescriptionContainsNul);
		}
	}
	Ok(())
}

pub enum WorkshopUpdateType {
	Description {
		description: String,
	},
	Creation {
		title: String,
		description: Option<String>,
		path: ContentPath,
		tags: Vec<String>,
		addon_type: String,
		preview: PathBuf,
		changes: Option<String>,
	},
	Update {
		// None preserves the current description; Some("") explicitly clears it.
		description: Option<String>,
		path: ContentPath,
		tags: Vec<String>,
		addon_type: String,
		preview: Option<PathBuf>,
		changes: Option<String>,
	},
}

impl Steam {
	pub fn update(&self, id: PublishedFileId, details: WorkshopUpdateType, transaction: &Transaction, job: &PublishJob) -> Result<bool, PublishError> {
		use WorkshopUpdateType::*;
		if transaction.aborted() {
			return Err(PublishError::Cancelled);
		}

		job.submitting().map_err(PublishError::Journal)?;
		let result = Arc::new(Mutex::new(None));
		let result_ref = result.clone();
		let update_handle = match details {
			Description { description } => {
				let update = self.client().ugc().start_item_update(GMOD_APP_ID, id).description(&description);
				if !transaction.begin_submission() {
					return Err(PublishError::Cancelled);
				}
				update.submit(None, move |result| {
					*result_ref.lock() = Some(result);
				})
			}
			Creation {
				title,
				description,
				path,
				mut tags,
				addon_type,
				preview,
				changes,
			} => {
				tags.reserve(tags.len() + 2);
				tags.push("Addon".to_string());
				tags.push(addon_type);

				let update = self
					.client()
					.ugc()
					.start_item_update(GMOD_APP_ID, id)
					.content_path(&path)
					.title(&title)
					.preview_path(&preview)
					.tags(tags, false);
				let update = match description {
					Some(description) => update.description(&description),
					None => update,
				};
				if transaction.aborted() {
					return Err(PublishError::Cancelled);
				}
				update.submit(changes.as_deref(), move |result| {
					*result_ref.lock() = Some(result);
				})
			}

			Update {
				description,
				path,
				tags,
				addon_type,
				preview,
				changes,
			} => {
				let mut tags = tags;
				tags.reserve(tags.len() + 2);
				tags.push("Addon".to_string());
				tags.push(addon_type);

				let preview_path = preview;

				let update = self.client().ugc().start_item_update(GMOD_APP_ID, id);
				let update = match description {
					Some(description) => update.description(&description),
					None => update,
				};
				let update = match preview_path {
					Some(preview_path) => update.preview_path(&preview_path),
					None => update,
				}
				.content_path(&path)
				.tags(tags, false);
				if !transaction.begin_submission() {
					return Err(PublishError::Cancelled);
				}
				update.submit(changes.as_deref(), move |result| {
					*result_ref.lock() = Some(result);
				})
			}
		};

		let mut last_progress = None;
		let mut last_change = std::time::Instant::now();
		let result = loop {
			let (processed, progress, total) = update_handle.progress();
			let current = (processed, progress, total);
			if last_progress != Some(current) { last_change = std::time::Instant::now(); }
			if last_change.elapsed().as_secs() >= 120 { job.uncertain(transaction); }
			if !matches!(processed, steamworks::UpdateStatus::Invalid) {
				transaction.status(match processed {
					steamworks::UpdateStatus::Invalid => unreachable!(),
					steamworks::UpdateStatus::PreparingConfig => "PUBLISH_PREPARING_CONFIG",
					steamworks::UpdateStatus::PreparingContent => "PUBLISH_PREPARING_CONTENT",
					steamworks::UpdateStatus::UploadingContent => "PUBLISH_UPLOADING_CONTENT",
					steamworks::UpdateStatus::UploadingPreviewFile => "PUBLISH_UPLOADING_PREVIEW_FILE",
					steamworks::UpdateStatus::CommittingChanges => "PUBLISH_COMMITTING_CHANGES",
				});
			}
			if total == 0 || last_progress.is_none_or(|(previous, _, _)| previous != processed) {
				transaction.progress_reset();
			} else {
				transaction.data(total);
				transaction.progress(progress as f64 / total as f64);
			}

			last_progress = Some(current);
			let reply = result.lock().take();
			if let Some(reply) = reply {
				break reply;
			} else {
				self.run_callbacks();
			}
		};

		match result {
			Ok((_, legal_agreement)) => {
				transaction.progress(1.);
				Ok(legal_agreement)
			}
			Err(SteamError::Timeout | SteamError::IOFailure | SteamError::RemoteDisconnect) => { job.uncertain(transaction); Err(PublishError::Unknown) },
			Err(error) => Err(PublishError::SteamError(error)),
		}
	}

	pub fn publish(&self, details: WorkshopUpdateType, transaction: &Transaction, job: &PublishJob) -> (Option<PublishedFileId>, Result<bool, PublishError>) {
		debug_assert!(matches!(details, WorkshopUpdateType::Creation { .. }));
		if transaction.aborted() {
			return (None, Err(PublishError::Cancelled));
		}

		if let Err(error) = job.submitting() { return (None, Err(PublishError::Journal(error))); }
		if !transaction.begin_submission() { return (None, Err(PublishError::Cancelled)); }
		let started = std::time::Instant::now();
		let published = Arc::new(Mutex::new(None));
		let published_ref = published.clone();
		self.client()
			.ugc()
			.create_item(GMOD_APP_ID, steamworks::FileType::Community, move |result| {
				*published_ref.lock() = Some(result);
			});

		let published = loop {
			if started.elapsed().as_secs() >= 120 { job.uncertain(transaction); }
			if let Some(reply) = published.lock().take() { break reply; }
			self.run_callbacks();
		};

		let id = match published {
			Ok((id, _)) => id,
			Err(SteamError::Timeout | SteamError::IOFailure | SteamError::RemoteDisconnect) => { job.uncertain(transaction); return (None, Err(PublishError::Unknown)); },
			Err(error) => return (None, Err(PublishError::SteamError(error))),
		};

		job.created(id, transaction);
		(Some(id), self.update(id, details, transaction, job))
	}

	pub fn update_icon(&self, addon_id: PublishedFileId, icon: PathBuf, transaction: &Transaction, job: &PublishJob) -> Result<bool, PublishError> {
		if transaction.aborted() {
			return Err(PublishError::Cancelled);
		}
		job.submitting().map_err(PublishError::Journal)?;
		let result = Arc::new(Mutex::new(None));
		let result_ref = result.clone();
		let update = self.client().ugc().start_item_update(GMOD_APP_ID, addon_id).preview_path(&icon);
		if !transaction.begin_submission() {
			return Err(PublishError::Cancelled);
		}
		let update_handle = update.submit(None, move |result| {
			*result_ref.lock() = Some(result);
		});

		let mut last_progress = None;
		let mut last_change = std::time::Instant::now();
		let result = loop {
			let (processed, progress, total) = update_handle.progress();
			let current = (processed, progress, total);
			if last_progress != Some(current) { last_change = std::time::Instant::now(); }
			if last_change.elapsed().as_secs() >= 120 { job.uncertain(transaction); }
			if !matches!(processed, steamworks::UpdateStatus::Invalid) {
				transaction.status(match processed {
					steamworks::UpdateStatus::Invalid => unreachable!(),
					steamworks::UpdateStatus::PreparingConfig => "PUBLISH_PREPARING_CONFIG",
					steamworks::UpdateStatus::PreparingContent => "PUBLISH_PREPARING_CONTENT",
					steamworks::UpdateStatus::UploadingContent => "PUBLISH_UPLOADING_CONTENT",
					steamworks::UpdateStatus::UploadingPreviewFile => "PUBLISH_UPLOADING_PREVIEW_FILE",
					steamworks::UpdateStatus::CommittingChanges => "PUBLISH_COMMITTING_CHANGES",
				});
			}
			if total == 0 || last_progress.is_none_or(|(previous, _, _)| previous != processed) {
				transaction.progress_reset();
			} else {
				transaction.data(total);
				transaction.progress(progress as f64 / total as f64);
			}

			last_progress = Some(current);
			let reply = result.lock().take();
			if let Some(reply) = reply {
				break reply;
			} else {
				self.run_callbacks();
			}
		};

		match result {
			Ok((_, legal_agreement)) => {
				transaction.progress(1.);
				Ok(legal_agreement)
			}
			Err(SteamError::Timeout | SteamError::IOFailure | SteamError::RemoteDisconnect) => { job.uncertain(transaction); Err(PublishError::Unknown) },
			Err(error) => Err(PublishError::SteamError(error)),
		}
	}
}

#[tauri::command]
pub fn verify_whitelist(path: PathBuf) -> Result<(Vec<GMAEntry>, u64), GMAError> {
	let ignore = app_data!().settings.read().ignore_globs.clone();
	ContentManifest::build(&path, &ignore, || false).map(ContentManifest::into_preview)
}

#[tauri::command]
pub fn publish_icon(icon_path: PathBuf, upscale: bool, addon_id: PublishedFileId) -> u32 {
	let transaction = crate::transactions::new_publish();
	let id = transaction.id;

	std::thread::spawn(move || {
		let job = match PublishJob::new(&transaction, Some(addon_id), None) { Ok(job) => job, Err(error) => return transaction.error(error, turbonone!()) };
		let temp_dir = app_data!().temp_dir().to_owned();
		let result = with_publish_staging(&temp_dir, |root, _| {
			job.staging(root)?;
			if transaction.aborted() {
				return Ok(None);
			}
			let icon = WorkshopIcon::new(icon_path, upscale).map_err(|error| error.to_string())?;
			if transaction.aborted() {
				return Ok(None);
			}
			let preview = icon.into_path(root).map_err(|error| error.to_string())?;
			if transaction.aborted() {
				return Ok(None);
			}
			match steam!().update_icon(addon_id, preview, &transaction, &job) {
				Ok(legal_agreement) => Ok(Some(legal_agreement)),
				Err(PublishError::Cancelled) => Ok(None),
				Err(error) => Err(error.to_string()),
			}
		});

		match result {
			Ok(StagingResult { value: Some(legal_agreement), warnings }) if !transaction.aborted() => {
				if legal_agreement {
					crate::path::open("https://steamcommunity.com/workshop/workshoplegalagreement");
				}
				transaction.finished(job.finish(&transaction, "published", warnings));
			}
			Ok(_) => { job.finish(&transaction, "cancelled", Vec::new()); transaction.cancelled(); },
			Err(error) if error == "ERR_PUBLISH_OUTCOME_UNKNOWN" => job.uncertain(&transaction),
			Err(error) => {
				let result = job.finish(&transaction, "failed", Vec::new());
				transaction.error(error, result);
			}
		};
	});

	id
}

#[tauri::command]
pub fn publish_description(addon_id: PublishedFileId, description: String) -> Result<u32, PublishError> {
	validate_description(Some(&description))?;
	let transaction = crate::transactions::new_publish();
	let id = transaction.id;

	std::thread::spawn(move || {
		let job = match PublishJob::new(&transaction, Some(addon_id), None) { Ok(job) => job, Err(error) => return transaction.error(error, turbonone!()) };
		transaction.status("PUBLISH_UPDATING_DESCRIPTION");
		match steam!().update(addon_id, WorkshopUpdateType::Description { description }, &transaction, &job) {
			Ok(legal_agreement) => {
				if legal_agreement {
					crate::path::open("https://steamcommunity.com/workshop/workshoplegalagreement");
				}
				transaction.finished(job.finish(&transaction, "published", Vec::new()));
			}
			Err(PublishError::Cancelled) => { job.finish(&transaction, "cancelled", Vec::new()); transaction.cancelled(); },
			Err(PublishError::Unknown) => job.uncertain(&transaction),
			Err(error) => { transaction.error(error.to_string(), job.finish(&transaction, "failed", Vec::new())); },
		}
	});

	Ok(id)
}

const DEFAULT_GMA_FILE_NAME: &str = "publishedaddon";
const GMA_FILE_NAME_MAX_CHARS: usize = 120;
const GMA_FILE_NAME_MAX_BYTES: usize = 251;

/// Turns an arbitrary string into a file name that is safe on every supported platform.
/// Returns None when nothing usable is left, so that callers can fall back to another name.
fn sanitize_gma_file_name(name: &str) -> Option<String> {
	let mut bytes = 0;
	let sanitized: String = name
		.trim()
		.chars()
		.filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
		.take(GMA_FILE_NAME_MAX_CHARS)
		.take_while(|c| {
			bytes += c.len_utf8();
			bytes <= GMA_FILE_NAME_MAX_BYTES
		})
		.collect();

	// Leave room for .gma and avoid Windows device names, even with multiple extensions.
	let sanitized = sanitized.trim().trim_end_matches(|c: char| c == '.' || c.is_whitespace());
	let base = sanitized.split('.').next().unwrap().trim_end().to_ascii_uppercase();
	let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
		|| base
			.strip_prefix("COM")
			.or_else(|| base.strip_prefix("LPT"))
			.is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"));

	if sanitized.is_empty() || sanitized.eq_ignore_ascii_case(".gma") || reserved {
		None
	} else {
		Some(sanitized.to_owned())
	}
}

/// Chooses the name of the .GMA file that gets packed and uploaded.
/// The chosen name wins, otherwise "publishedaddon.gma" is used.
fn resolve_gma_file_name(gma_name: Option<&str>) -> String {
	let mut file_name = gma_name
		.and_then(sanitize_gma_file_name)
		.unwrap_or_else(|| DEFAULT_GMA_FILE_NAME.to_owned());

	if file_name.to_ascii_lowercase().ends_with(".gma") {
		file_name.truncate(file_name.len() - 4);
	}
	file_name.push_str(".gma");

	file_name
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishRequest {
	content_path_src: PathBuf,
	icon_path: Option<PathBuf>,
	title: String,
	description: Option<String>,
	tags: Vec<String>,
	addon_type: String,
	upscale: bool,
	update_id: Option<PublishedFileId>,
	changes: Option<String>,
	gma_name: Option<String>,
}

#[tauri::command]
pub fn publish(request: PublishRequest) -> u32 {
	let PublishRequest {
		content_path_src,
		icon_path,
		title,
		description,
		tags,
		addon_type,
		upscale,
		update_id,
		changes,
		gma_name,
	} = request;
	let transaction = crate::transactions::new_publish();
	let id = transaction.id;

	let is_updating = update_id.is_some();

	std::thread::spawn(move || {
		let job = match PublishJob::new(&transaction, update_id, Some(content_path_src.clone())) { Ok(job) => job, Err(error) => return transaction.error(error, turbonone!()) };
		let source_key = if is_updating {
			None
		} else {
			match crate::appdata::changelog_key(None, Some(content_path_src.clone())) {
				Ok(key) => Some(key),
				Err(error) => {
					transaction.error(error, job.finish(&transaction, "failed", Vec::new()));
					return;
				}
			}
		};
		let temp_dir = app_data!().temp_dir().to_owned();
		let result = with_publish_staging(&temp_dir, |root, content| {
			job.staging(root)?;
			validate_description(description.as_deref()).map_err(|error| error.to_string())?;
			if transaction.aborted() {
				return Ok(None);
			}
			let ignore = app_data!().settings.read().ignore_globs.clone();
			// The preview may be stale by the time this worker starts.
			let manifest = match ContentManifest::build(&content_path_src, &ignore, || transaction.aborted()) {
				Ok(manifest) => manifest,
				Err(GMAError::Cancelled) => return Ok(None),
				Err(error) => return Err(error.to_string()),
			};

			let preview = match icon_path {
				Some(icon_path) => {
					transaction.status("PUBLISH_PROCESSING_ICON");
					Some(WorkshopIcon::new(icon_path, upscale).map_err(|error| error.to_string())?)
				}
				None if !is_updating => Some(WorkshopIcon::Default),
				None => None,
			};
			if transaction.aborted() {
				return Ok(None);
			}
			let preview = preview.map(|icon| icon.into_path(root)).transpose().map_err(|error| error.to_string())?;

			if !transaction.begin_packing() {
				return Ok(None);
			}
			transaction.status("PUBLISH_PACKING");

			let gma = GMAFile {
				// TODO convert to GMAFile::new()
				path: content.join(resolve_gma_file_name(gma_name.as_deref())),
				size: 0,
				id: None,
				metadata: Some(GMAMetadata::Standard {
					title: title.clone(),
					addon_type: addon_type.clone(),
					tags: tags.clone(),
					ignore,
				}),
				entries: None,
				pointers: GMAFilePointers::default(),
				version: 3,
				extracted_name: String::new(),
				modified: None,
				membuffer: None,
				spool: None,
			};

			if let Err(error) = gma.create(manifest, transaction.clone()) {
				if matches!(error, crate::gma::GMAError::Cancelled) {
					return Ok(None);
				}
				return Err(error.to_string());
			}
			if transaction.aborted() {
				return Ok(None);
			}
			let content_path = ContentPath::new(content.to_owned()).map_err(|error| error.to_string())?;

			transaction.status("PUBLISH_STARTING");

			let (id, result) = if let Some(id) = update_id {
				(
					update_id,
					steam!().update(
						id,
						WorkshopUpdateType::Update {
							description,
							path: content_path,
							tags,
							addon_type,
							preview,
							changes,
						},
						&transaction,
						&job,
					),
				)
			} else {
				steam!().publish(
					WorkshopUpdateType::Creation {
						title,
						description,
						path: content_path,
						tags,
						addon_type,
						preview: preview.unwrap(),
						changes,
					},
					&transaction,
					&job,
				)
			};
			match result {
				Ok(legal_agreement) => Ok(Some((id.unwrap(), legal_agreement))),
				Err(error) => {
					if matches!(error, PublishError::Cancelled) {
						Ok(None)
					} else {
						Err(error.to_string())
					}
				}
			}
		});

		match result {
			Ok(StagingResult { value: Some((id, legal_agreement)), warnings }) => {
				let settings_error = crate::appdata::change_settings(|settings| {
					settings.my_workshop_local_paths.insert(id, content_path_src);
					if let Some(key) = source_key { settings.changelogs.published(&key, id); }
					Ok(())
				}).err();
				app_data!().send();
				if !transaction.aborted() {
					if legal_agreement {
						crate::path::open("https://steamcommunity.com/workshop/workshoplegalagreement");
					}
					if app_data!().settings.read().open_workshop_after_publish {
						crate::path::open(format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", id.0));
					}
					let mut result = serde_json::to_value(job.finish(&transaction, "published", warnings)).unwrap();
					result["settingsError"] = json!(settings_error);
					transaction.finished(result);
				}
			}
			Ok(StagingResult { value: None, .. }) => { job.finish(&transaction, "cancelled", Vec::new()); transaction.cancelled(); },
			Err(error) if error == "ERR_PUBLISH_OUTCOME_UNKNOWN" => job.uncertain(&transaction),
			Err(error) => transaction.error(error, job.finish(&transaction, "failed", Vec::new())),
		};
	});

	id
}

#[tauri::command]
pub fn verify_icon(path: PathBuf) -> Result<(String, bool), Transaction> {
	WorkshopIcon::new(&path, false)
		.and_then(|icon| {
			let (prefix, can_upscale) = match icon {
				WorkshopIcon::Custom { format, width, height, .. } => (
					format!(
						"data:image/{};base64,",
						match format {
							ImageFormat::Png => "png",
							ImageFormat::Jpeg => "jpeg",
							ImageFormat::Gif => "gif",
							_ => unreachable!(),
						}
					),
					WorkshopIcon::can_upscale(width, height, format),
				),
				_ => unreachable!(),
			};
			let base64 = base64::encode(std::fs::read(&path).map_err(|error| PublishError::io("read icon", &path, error))?);
			Ok((prefix + &base64, can_upscale))
		})
		.map_err(|error| {
			let transaction = transaction!();
			transaction.error(error.to_string(), turbonone!());
			transaction
		})
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;

	#[test]
	fn overlapping_jobs_isolate_archives_and_generated_icons() {
		let parent = tempfile::tempdir().unwrap();
		let source = parent.path().join("source.png");
		DynamicImage::new_rgb8(32, 32).save(&source).unwrap();
		let first_root = with_publish_staging(parent.path(), |first_root, first_content| {
			let archive = first_content.join("addon.gma");
			fs::write(&archive, b"first job").unwrap();
			let first_icon = WorkshopIcon::new(&source, true).unwrap().into_path(first_root).unwrap();
			assert_eq!(first_icon.parent(), Some(first_root));
			let content = ContentPath::new(first_content.to_owned()).unwrap();
			assert_eq!(&**content, first_content);
			assert_eq!(fs::read_dir(&*content).unwrap().count(), 1);

			let second_root = with_publish_staging(parent.path(), |second_root, second_content| {
				assert_ne!(first_root, second_root);
				fs::write(second_content.join("addon.gma"), b"second job").unwrap();
				let second_icon = WorkshopIcon::new(&source, true).unwrap().into_path(second_root).unwrap();
				assert_ne!(first_icon, second_icon);
				assert_eq!(fs::read(&archive).unwrap(), b"first job");
				assert_eq!(fs::read(second_content.join("addon.gma")).unwrap(), b"second job");
				assert_eq!(&**ContentPath::new(second_content.to_owned()).unwrap(), second_content);
				Ok(second_root.to_owned())
			})?;
			assert!(!second_root.value.exists());
			assert!(archive.is_file());
			assert!(first_icon.is_file());
			Ok(first_root.to_owned())
		})
		.unwrap();
		assert!(!first_root.value.exists());
		assert!(source.is_file());
		parent.close().unwrap();
	}

	#[test]
	fn staging_cleans_up_after_failure_and_cancellation() {
		let parent = tempfile::tempdir().unwrap();
		for failure in [true, false] {
			let mut staged_root = PathBuf::new();
			let result = with_publish_staging(parent.path(), |root, content| {
				staged_root = root.to_owned();
				fs::write(content.join("partial.gma"), b"partial archive").unwrap();
				fs::write(root.join("default_workshop_icon.png"), b"generated preview").unwrap();
				if failure {
					return Err("ERR_STEAM_ERROR:upload failed".to_owned());
				}
				Ok(None::<()>)
			});
			if failure {
				assert_eq!(result.unwrap_err(), "ERR_STEAM_ERROR:upload failed");
			} else {
				assert_eq!(result.unwrap().value, None);
			}
			assert!(!staged_root.exists());
		}
		parent.close().unwrap();
	}

	#[test]
	fn staging_cleans_up_after_icon_and_content_validation_errors() {
		let parent = tempfile::tempdir().unwrap();
		for invalid_icon in [true, false] {
			let mut staged_root = PathBuf::new();
			let result = with_publish_staging(parent.path(), |root, content| {
				staged_root = root.to_owned();
				fs::write(content.join("addon.gma"), b"archive").unwrap();
				if invalid_icon {
					WorkshopIcon::new(root.join("missing.png"), true).map_err(|error| error.to_string())?;
				} else {
					fs::write(content.join("second.gma"), b"another archive").unwrap();
					ContentPath::new(content.to_owned()).map_err(|error| error.to_string())?;
				}
				Ok(())
			});
			assert!(result
				.unwrap_err()
				.contains(if invalid_icon { "read icon metadata" } else { "ERR_MULTIPLE_GMAS" }));
			assert!(!staged_root.exists());
		}
		parent.close().unwrap();
	}

	#[cfg(target_os = "windows")]
	#[test]
	fn cleanup_failures_preserve_the_operation_error_and_path() {
		use std::os::windows::fs::OpenOptionsExt;
		let parent = tempfile::tempdir().unwrap();
		for failure in [true, false] {
			let mut lock = None;
			let mut staged_root = PathBuf::new();
			let result = with_publish_staging(parent.path(), |root, content| {
				staged_root = root.to_owned();
				lock = Some(
					fs::OpenOptions::new()
						.write(true)
						.create_new(true)
						.share_mode(0)
						.open(content.join("locked.gma"))
						.unwrap(),
				);
				if failure {
					Err("ERR_STEAM_ERROR:upload failed".to_owned())
				} else {
					Ok(())
				}
			});
			let error = if failure { result.unwrap_err() } else {
				let successful = result.unwrap();
				assert_eq!(successful.value, ());
				successful.warnings.join("\n")
			};
			assert!(error.contains("remove publishing staging directory"));
			assert!(error.contains(staged_root.to_str().unwrap()));
			if failure { assert!(error.contains("ERR_STEAM_ERROR:upload failed")); }
			drop(lock);
		}
		parent.close().unwrap();
	}

	#[test]
	fn content_requires_exactly_one_regular_gma_and_returns_its_directory() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path();
		assert!(matches!(ContentPath::new(path.to_owned()), Err(PublishError::NoEntries)));
		let archive = path.join("addon.GMA");
		fs::create_dir(&archive).unwrap();
		assert!(matches!(ContentPath::new(path.to_owned()), Err(PublishError::InvalidContentPath)));
		fs::remove_dir(&archive).unwrap();
		fs::write(&archive, b"archive").unwrap();
		assert_eq!(&**ContentPath::new(path.to_owned()).unwrap(), path);
		let extra = path.join("preview.png");
		fs::write(&extra, b"icon").unwrap();
		assert!(matches!(ContentPath::new(path.to_owned()), Err(PublishError::InvalidContentPath)));
		fs::remove_file(extra).unwrap();
		fs::write(path.join("second.gma"), b"archive").unwrap();
		assert!(matches!(ContentPath::new(path.to_owned()), Err(PublishError::MultipleGMAs)));
		directory.close().unwrap();
	}

	#[test]
	fn generated_names_are_sanitized_and_accepted_as_content() {
		let directory = tempfile::tempdir().unwrap();
		for (input, expected) in [
			(None, "publishedaddon.gma"),
			(Some(""), "publishedaddon.gma"),
			(Some(" /\\:*?\"<>|\0\n . . "), "publishedaddon.gma"),
			(Some(".GMA"), "publishedaddon.gma"),
			(Some("NUL.tar.gma"), "publishedaddon.gma"),
			(Some("con .gma"), "publishedaddon.gma"),
			(Some("prn"), "publishedaddon.gma"),
			(Some("AUX"), "publishedaddon.gma"),
			(Some("COM1"), "publishedaddon.gma"),
			(Some("lpt9.GMA"), "publishedaddon.gma"),
			(Some("COM¹"), "publishedaddon.gma"),
			(Some("LPT²"), "publishedaddon.gma"),
			(Some("COM³"), "publishedaddon.gma"),
			(Some("COM10"), "COM10.gma"),
			(Some(" My:Addon?.GmA  "), "MyAddon.gma"),
			(Some("name. . "), "name.gma"),
			(Some("../folder\\addon\0"), "..folderaddon.gma"),
		] {
			let name = resolve_gma_file_name(input);
			assert_eq!(name, expected, "{input:?}");
			assert_eq!(Path::new(&name).components().count(), 1);
			let archive = directory.path().join(name);
			fs::write(&archive, b"archive").unwrap();
			assert!(ContentPath::new(directory.path().to_owned()).is_ok());
			fs::remove_file(archive).unwrap();
		}
		for input in ["a".repeat(130), "🦀".repeat(120), "界".repeat(120)] {
			let name = resolve_gma_file_name(Some(&input));
			assert!(name.len() <= 255);
			assert!(name.chars().count() <= GMA_FILE_NAME_MAX_CHARS + 4);
			assert!(name.ends_with(".gma"));
			let archive = directory.path().join(name);
			fs::write(&archive, b"archive").unwrap();
			assert!(ContentPath::new(directory.path().to_owned()).is_ok());
			fs::remove_file(archive).unwrap();
		}
		directory.close().unwrap();
	}

	#[test]
	fn publish_request_accepts_frontend_fields_and_optional_updates() {
		let mut payload = serde_json::json!({
			"contentPathSrc": "addons/example",
			"title": "Example",
			"tags": ["fun"],
			"addonType": "tool",
			"upscale": false,
		});
		let request: PublishRequest = serde_json::from_value(payload.clone()).unwrap();
		assert_eq!(request.content_path_src, std::path::Path::new("addons/example"));
		assert_eq!(request.title, "Example");
		assert_eq!(request.tags, ["fun"]);
		assert_eq!(request.addon_type, "tool");
		assert!(!request.upscale);
		assert!(request.description.is_none());
		assert!(request.update_id.is_none());
		assert!(request.icon_path.is_none());
		assert!(request.changes.is_none());
		assert!(request.gma_name.is_none());

		payload["description"] = serde_json::json!("");
		payload["updateId"] = serde_json::json!(PublishedFileId(42));
		payload["iconPath"] = serde_json::json!("preview.png");
		payload["changes"] = serde_json::json!("[b]Updated[/b]");
		payload["gmaName"] = serde_json::json!("example.gma");
		let request: PublishRequest = serde_json::from_value(payload.clone()).unwrap();
		assert_eq!(request.description.as_deref(), Some(""));
		assert_eq!(request.update_id, Some(PublishedFileId(42)));
		assert_eq!(request.icon_path.as_deref(), Some(std::path::Path::new("preview.png")));
		assert_eq!(request.changes.as_deref(), Some("[b]Updated[/b]"));
		assert_eq!(request.gma_name.as_deref(), Some("example.gma"));

		payload.as_object_mut().unwrap().remove("title");
		assert!(serde_json::from_value::<PublishRequest>(payload).is_err());
	}

	#[test]
	fn icon_output_errors_are_returned_with_context() {
		use super::*;
		let root = std::env::temp_dir().join(format!("nwmpublisher-icon-errors-{}", std::process::id()));
		std::fs::create_dir(&root).unwrap();
		let source = root.join("source.png");
		DynamicImage::new_rgb8(32, 32).save(&source).unwrap();
		let icon = WorkshopIcon::new(&source, true).unwrap();
		let error = icon.into_path(&root.join("missing")).unwrap_err();
		assert!(error.to_string().contains("create icon"));
		assert!(error.to_string().contains("nwmpublisher_upscaled_icon.png"));
		assert!(source.is_file());
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn icon_encoder_propagates_write_and_flush_errors() {
		use super::*;
		use std::io;
		struct FailingWriter(bool);
		impl Write for FailingWriter {
			fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
				if self.0 {
					Err(io::Error::new(io::ErrorKind::WriteZero, "disk full"))
				} else {
					Ok(bytes.len())
				}
			}
			fn flush(&mut self) -> io::Result<()> {
				Err(io::Error::other("flush failed"))
			}
		}
		let image = DynamicImage::new_rgb8(2, 2);
		for (fail_write, operation, message) in [(true, "encode icon", "disk full"), (false, "flush icon", "flush failed")] {
			let error = encode_icon(&image, &mut FailingWriter(fail_write), Path::new("icon.png"), ImageFormat::Png).unwrap_err();
			assert!(error.to_string().contains(operation));
			assert!(error.to_string().contains("icon.png"));
			assert!(error.to_string().contains(message));
			assert_eq!(serde_json::to_value(&error).unwrap(), error.to_string());
		}
	}
	use steamworks::PublishedFileId;

	#[test]
	fn description_update_rejects_invalid_input_before_starting() {
		assert!(matches!(
			publish_description(PublishedFileId(0), "a".repeat(WORKSHOP_DESCRIPTION_MAX_BYTES + 1)),
			Err(PublishError::DescriptionTooLong)
		));
		assert!(matches!(
			publish_description(PublishedFileId(0), "before\0after".to_owned()),
			Err(PublishError::DescriptionContainsNul)
		));
	}

	#[test]
	fn description_can_be_omitted_or_explicitly_empty() {
		assert!(validate_description(None).is_ok());
		assert!(validate_description(Some("")).is_ok());
	}

	#[test]
	fn description_accepts_plain_text_bbcode_and_whitespace() {
		assert!(validate_description(Some("  [b]Title[/b]\n\n[code]print('hello')[/code]\n  ")).is_ok());
		assert!(validate_description(Some("\n\t ")).is_ok());
	}

	#[test]
	fn description_reserves_space_for_the_nul_terminator() {
		let description = "a".repeat(WORKSHOP_DESCRIPTION_MAX_BYTES);
		assert!(validate_description(Some(&description)).is_ok());
		assert!(matches!(
			validate_description(Some(&(description + "a"))),
			Err(PublishError::DescriptionTooLong)
		));
	}

	#[test]
	fn description_limit_counts_utf8_bytes() {
		let description = "🦀".repeat(WORKSHOP_DESCRIPTION_MAX_BYTES / 4) + "abc";
		assert_eq!(description.len(), WORKSHOP_DESCRIPTION_MAX_BYTES);
		assert!(validate_description(Some(&description)).is_ok());
		assert!(matches!(
			validate_description(Some(&(description + "é"))),
			Err(PublishError::DescriptionTooLong)
		));
	}

	#[test]
	fn description_rejects_nul_anywhere() {
		for description in ["\0text", "before\0after", "text\0"] {
			assert!(matches!(
				validate_description(Some(description)),
				Err(PublishError::DescriptionContainsNul)
			));
		}
	}
}
