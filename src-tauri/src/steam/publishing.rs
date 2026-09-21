use crate::{
	gma::{GMAEntry, GMAFile, GMAFilePointers, GMAMetadata},
	Transaction, GMOD_APP_ID,
};
use image::{DynamicImage, GenericImageView, ImageError, ImageFormat};
use parking_lot::Mutex;
use path_slash::PathExt;
use std::{
	fs::File,
	io::{BufReader, BufWriter, Write},
	path::{Path, PathBuf},
	sync::Arc,
};
use steamworks::{PublishedFileId, SteamError};
use walkdir::WalkDir;

#[cfg(not(target_os = "windows"))]
use std::collections::HashSet;

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
	NotWhitelisted(Vec<String>),
	NoEntries,
	DuplicateEntry(String),
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
			PublishError::NotWhitelisted(whitelisted) => write!(f, "ERR_WHITELIST:{}", whitelisted.join("\n")),
			PublishError::NoEntries => write!(f, "ERR_NO_ENTRIES"),
			PublishError::DuplicateEntry(path) => write!(f, "ERR_DUPLICATE_ENTRIES:{}", path),
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

use super::Steam;
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

		let mut gma_path: Option<PathBuf> = None;
		for entry in path
			.read_dir()
			.map_err(|error| PublishError::io("read content directory", &path, error))?
		{
			let entry = entry.map_err(|error| PublishError::io("read directory entry", &path, error))?;
			let path = entry.path();
			if path.extension().is_none_or(|extension| extension != "gma") {
				continue;
			}

			if gma_path.is_some() {
				return Err(PublishError::MultipleGMAs);
			}
			gma_path = Some(path);
		}

		gma_path.map(ContentPath).ok_or(PublishError::NoEntries)
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
	pub fn update(&self, id: PublishedFileId, details: WorkshopUpdateType, transaction: &Transaction) -> Result<bool, PublishError> {
		use WorkshopUpdateType::*;

		let result = Arc::new(Mutex::new(None));
		let result_ref = result.clone();
		let update_handle = match details {
			Description { description } => {
				self.client()
					.ugc()
					.start_item_update(GMOD_APP_ID, id)
					.description(&description)
					.submit(None, move |result| {
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
				match preview_path {
					Some(preview_path) => update.preview_path(&preview_path),
					None => update,
				}
				.content_path(&path)
				.tags(tags, false)
				.submit(changes.as_deref(), move |result| {
					*result_ref.lock() = Some(result);
				})
			}
		};

		let mut last_processed;
		let result = loop {
			let (processed, progress, total) = update_handle.progress();
			last_processed = processed;
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
			if total == 0 || last_processed != processed {
				transaction.progress_reset();
			} else {
				transaction.data(total);
				transaction.progress(progress as f64 / total as f64);
			}

			if !result.is_locked() && result.lock().is_some() {
				break Arc::try_unwrap(result).unwrap().into_inner().unwrap();
			} else {
				self.run_callbacks();
			}
		};

		match result {
			Ok((_, legal_agreement)) => {
				transaction.progress(1.);
				Ok(legal_agreement)
			}
			Err(error) => Err(PublishError::SteamError(error)),
		}
	}

	pub fn publish(&self, details: WorkshopUpdateType, transaction: &Transaction) -> (Option<PublishedFileId>, Result<bool, PublishError>) {
		debug_assert!(matches!(details, WorkshopUpdateType::Creation { .. }));

		let published = Arc::new(Mutex::new(None));
		let published_ref = published.clone();
		self.client()
			.ugc()
			.create_item(GMOD_APP_ID, steamworks::FileType::Community, move |result| {
				*published_ref.lock() = Some(result);
			});

		loop {
			if let Some(published_ref) = published.try_lock() {
				if published_ref.is_some() {
					break;
				}
			}
			self.run_callbacks();
		}

		let id = match Arc::try_unwrap(published).unwrap().into_inner().unwrap() {
			Ok((id, _)) => id,
			Err(error) => return (None, Err(PublishError::SteamError(error))),
		};

		(Some(id), self.update(id, details, transaction))
	}

	pub fn update_icon(&self, addon_id: PublishedFileId, icon: PathBuf, transaction: &Transaction) -> Result<bool, PublishError> {
		let result = Arc::new(Mutex::new(None));
		let result_ref = result.clone();
		let update_handle = self
			.client()
			.ugc()
			.start_item_update(GMOD_APP_ID, addon_id)
			.preview_path(&icon)
			.submit(None, move |result| {
				*result_ref.lock() = Some(result);
			});

		let mut last_processed;
		let result = loop {
			let (processed, progress, total) = update_handle.progress();
			last_processed = processed;
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
			if total == 0 || last_processed != processed {
				transaction.progress_reset();
			} else {
				transaction.data(total);
				transaction.progress(progress as f64 / total as f64);
			}

			if !result.is_locked() && result.lock().is_some() {
				break Arc::try_unwrap(result).unwrap().into_inner().unwrap();
			} else {
				self.run_callbacks();
			}
		};

		match result {
			Ok((_, legal_agreement)) => {
				transaction.progress(1.);
				Ok(legal_agreement)
			}
			Err(error) => Err(PublishError::SteamError(error)),
		}
	}
}

#[tauri::command]
pub fn verify_whitelist(path: PathBuf) -> Result<(Vec<GMAEntry>, u64), PublishError> {
	if !path.is_absolute()
		|| !path
			.metadata()
			.map_err(|error| PublishError::io("read content metadata", &path, error))?
			.is_dir()
	{
		return Err(PublishError::InvalidContentPath);
	}

	let content_root = path.clone();

	let ignore = app_data!().settings.read().ignore_globs.clone();

	let mut size = 0;
	let mut failed_extra = false;
	let mut failed = Vec::with_capacity(10);
	let mut files = Vec::new();

	#[cfg(not(target_os = "windows"))]
	let mut dedup: HashSet<String> = HashSet::new();

	for entry in WalkDir::new(&path).follow_links(false).contents_first(true) {
		let entry = entry.map_err(|error| {
			let path = error.path().unwrap_or(&content_root).to_owned();
			PublishError::io("read content directory", &path, error.into())
		})?;
		if entry.path_is_symlink() || entry.file_type().is_dir() {
			continue;
		}
		let path = entry.into_path();
		let relative_path = path
			.strip_prefix(&content_root)
			.map_err(|_| PublishError::InvalidContentPath)?
			.to_slash_lossy()
			.to_string();
		if !crate::gma::whitelist::filter_default_ignored(&relative_path) || crate::gma::whitelist::is_ignored(&relative_path, &ignore) {
			continue;
		}

		#[cfg(not(target_os = "windows"))]
		{
			if !dedup.insert(relative_path.to_owned()) {
				return Err(PublishError::DuplicateEntry(relative_path));
			}
		}

		if !crate::gma::whitelist::check(&relative_path) {
			if failed.len() == 9 {
				failed_extra = true;
				break;
			} else {
				failed.push(relative_path);
			}
		} else if failed.is_empty() {
			let entry_size = path
				.metadata()
				.map_err(|error| PublishError::io("read source metadata", &path, error))?
				.len();
			size += entry_size;
			files.push(GMAEntry {
				path: relative_path,
				size: entry_size,
				crc: 0,
				index: 0,
			});
		}
	}

	// TODO some tasks shouldnt be cancelable (i.e. showing the cross button)

	if failed.is_empty() {
		if files.is_empty() {
			Err(PublishError::NoEntries)
		} else {
			Ok((files, size))
		}
	} else {
		failed.sort_unstable();

		if failed_extra {
			failed.push("...".to_string());
		}

		Err(PublishError::NotWhitelisted(failed))
	}
}

#[tauri::command]
pub fn publish_icon(icon_path: PathBuf, upscale: bool, addon_id: PublishedFileId) -> u32 {
	let transaction = transaction!();
	let id = transaction.id;

	rayon::spawn(move || {
		let preview = match WorkshopIcon::new(icon_path, upscale).and_then(|icon| icon.into_path(&app_data!().temp_dir())) {
			Ok(icon) => icon,
			Err(error) => {
				transaction.error(error.to_string(), turbonone!());
				return;
			}
		};

		let result = steam!().update_icon(addon_id, preview, &transaction);

		match result {
			Ok(legal_agreement) => {
				if legal_agreement {
					crate::path::open("https://steamcommunity.com/workshop/workshoplegalagreement");
				}
				transaction.finished(turbonone!());
			}
			Err(error) => {
				transaction.error(error.to_string(), turbonone!());
			}
		};
	});

	id
}

#[tauri::command]
pub fn publish_description(addon_id: PublishedFileId, description: String) -> Result<u32, PublishError> {
	validate_description(Some(&description))?;
	let transaction = transaction!();
	let id = transaction.id;

	rayon::spawn(move || {
		transaction.status("PUBLISH_UPDATING_DESCRIPTION");
		match steam!().update(addon_id, WorkshopUpdateType::Description { description }, &transaction) {
			Ok(legal_agreement) => {
				if legal_agreement {
					crate::path::open("https://steamcommunity.com/workshop/workshoplegalagreement");
				}
				transaction.finished(turbonone!());
			}
			Err(error) => transaction.error(error.to_string(), turbonone!()),
		}
	});

	Ok(id)
}

const DEFAULT_GMA_FILE_NAME: &str = "publishedaddon";
const GMA_FILE_NAME_MAX_CHARS: usize = 120;

/// Turns an arbitrary string into a file name that is safe on every supported platform.
/// Returns None when nothing usable is left, so that callers can fall back to another name.
fn sanitize_gma_file_name(name: &str) -> Option<String> {
	let sanitized: String = name
		.trim()
		.chars()
		.filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
		.take(GMA_FILE_NAME_MAX_CHARS)
		.collect();

	// Windows rejects file names that end in a dot or a space
	let sanitized = sanitized.trim().trim_end_matches('.').trim_end();

	if sanitized.is_empty() {
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

	if !file_name.to_ascii_lowercase().ends_with(".gma") {
		file_name.push_str(".gma");
	}

	file_name
}

#[tauri::command]
pub fn publish(
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
) -> u32 {
	let transaction = transaction!();
	let id = transaction.id;

	let is_updating = update_id.is_some();

	rayon::spawn(move || {
		if let Err(error) = validate_description(description.as_deref()) {
			transaction.error(error.to_string(), turbonone!());
			return;
		}

		let preview = match icon_path {
			Some(icon_path) => {
				transaction.status("PUBLISH_PROCESSING_ICON");

				match WorkshopIcon::new(icon_path, upscale) {
					Ok(icon) => Some(icon),
					Err(error) => {
						transaction.error(error.to_string(), turbonone!());
						return;
					}
				}
			}
			None => {
				if !is_updating {
					Some(WorkshopIcon::Default)
				} else {
					None
				}
			}
		};

		let preview = match preview.map(|icon| icon.into_path(&app_data!().temp_dir())).transpose() {
			Ok(preview) => preview,
			Err(error) => {
				transaction.error(error.to_string(), turbonone!());
				return;
			}
		};

		transaction.status("PUBLISH_PACKING");

		let mut path = app_data!().temp_dir().to_owned();
		path.pop();
		path.push("nwmpublisher_publishing");

		if let Err(error) = std::fs::create_dir_all(&path) {
			transaction.error(PublishError::io("create publishing directory", &path, error).to_string(), turbonone!());
			return;
		}

		path.push(resolve_gma_file_name(gma_name.as_deref()));

		{
			let gma = GMAFile {
				// TODO convert to GMAFile::new()
				path: path.clone(),
				size: 0,
				id: None,
				metadata: Some(GMAMetadata::Standard {
					title: title.clone(),
					addon_type: addon_type.clone(),
					tags: tags.clone(),
					ignore: app_data!().settings.read().ignore_globs.clone(),
				}),
				entries: None,
				pointers: GMAFilePointers::default(),
				version: 3,
				extracted_name: String::new(),
				modified: None,
				membuffer: None,
			};

			if let Err(error) = gma.create(&content_path_src, transaction.clone()) {
				if !transaction.aborted() {
					transaction.error(error.to_string(), turbonone!());
				}
				return;
			}
		}

		let mut content_path = path.clone();
		content_path.pop();

		let content_path = match ContentPath::new(content_path) {
			Ok(content_path) => content_path,
			Err(error) => {
				transaction.error(error.to_string(), turbonone!());
				return;
			}
		};

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
			)
		};

		ignore! { std::fs::remove_file(path) };

		match result {
			Ok(legal_agreement) => {
				if legal_agreement {
					crate::path::open("https://steamcommunity.com/workshop/workshoplegalagreement");
				}

				let id = id.unwrap();

				crate::path::open(format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", id.0));

				transaction.finished(turbonone!());

				app_data!().settings.write().my_workshop_local_paths.insert(id, content_path_src);
				ignore! { app_data!().settings.read().save() };
				app_data!().send();
			}
			Err(error) => {
				transaction.error(error.to_string(), turbonone!());
				if !is_updating {
					if let Some(id) = id {
						steam!().client().ugc().delete_item(id, |_| {});
					}
				}
			}
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
	use super::{publish_description, validate_description, PublishError, WORKSHOP_DESCRIPTION_MAX_BYTES};

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
				Err(io::Error::new(io::ErrorKind::Other, "flush failed"))
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
