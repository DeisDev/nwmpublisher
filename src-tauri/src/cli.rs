use std::{
	io::Write,
	path::{Path, PathBuf},
	process::ExitCode,
	time::{Duration, Instant},
};

use clap::{builder::PossibleValuesParser, Arg, ArgAction, ArgGroup, ArgMatches, Command};
use steamworks::PublishedFileId;

use crate::{
	gma::{
		filename::{valid_metadata_file_name, DEFAULT_METADATA_FILE_NAME},
		write::{package_archive, PackageRequest, ADDON_TAGS, ADDON_TYPES},
		ExtractDestination, ExtractGMAMut, ExtractOptions,
	},
	logging::Level,
	steam::publishing::{run_publish, PublishOutcome, PublishRequest},
	util::english,
	GMAError, GMAFile, IoError,
};

pub static CLI_MODE: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::args_os().len() > 1);

const STEAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_TAGS: usize = 3;
const ACTIONS: [&str; 5] = ["extract", "pack", "list", "info", "publish"];

#[cfg(target_os = "windows")]
fn attach_console() {
	#[link(name = "kernel32")]
	unsafe extern "system" {
		fn AttachConsole(process_id: u32) -> i32;
	}

	// Release builds use the Windows GUI subsystem. A parent console is optional
	// (file associations have none); debug builds may already be attached.
	unsafe { AttachConsole(u32::MAX) };
}

fn failure_details(error: GMAError) -> String {
	match error {
		GMAError::IOError(error) => error.to_string(),
		GMAError::InvalidHeader => "invalid GMA header (expected GMAD)".into(),
		GMAError::FormatError => "invalid GMA archive format".into(),
		GMAError::MetadataError(error) => format!("invalid GMA metadata: {error}"),
		error => error.to_string(),
	}
}

fn metadata_name(name: &str) -> Result<String, String> {
	if valid_metadata_file_name(name) {
		Ok(name.to_owned())
	} else {
		Err(english::text("ERR_METADATA_FILE_NAME").to_owned())
	}
}

// Clap waives `requires` when a conflicting action is present, so options conflict with the actions they do not apply to.
fn only_with(arg: Arg, actions: &[&str]) -> Arg {
	arg.conflicts_with_all(ACTIONS.into_iter().filter(|action| !actions.contains(action)))
}

fn command() -> Command {
	Command::new("nwmpublisher")
		.version(env!("CARGO_PKG_VERSION"))
		.author("William Venner <william@venner.io>")
		.about(english::text("cli.about"))
		.after_help("Run without arguments to launch the GUI.\nExit codes: 0 = success, 1 = operation failed, 2 = invalid arguments.")
		.args([
			Arg::new("extract")
				.short('e')
				.long("extract")
				.value_name("FILE")
				.value_parser(clap::value_parser!(PathBuf))
				.help("Extract a .GMA file"),
			Arg::new("pack")
				.long("pack")
				.value_name("FOLDER")
				.value_parser(clap::value_parser!(PathBuf))
				.help(english::text("cli.pack")),
			Arg::new("list")
				.long("list")
				.value_name("FILE")
				.value_parser(clap::value_parser!(PathBuf))
				.help(english::text("cli.list")),
			Arg::new("info")
				.long("info")
				.value_name("FILE")
				.value_parser(clap::value_parser!(PathBuf))
				.help(english::text("cli.info")),
			Arg::new("publish")
				.long("publish")
				.value_name("FOLDER")
				.value_parser(clap::value_parser!(PathBuf))
				.help(english::text("cli.publish")),
			only_with(Arg::new("out"), &["extract", "pack"])
				.short('o')
				.long("out")
				.value_name("PATH")
				.value_parser(clap::value_parser!(PathBuf))
				.conflicts_with_all(["out-parent", "workshop-title"])
				.help(english::text("cli.out")),
			only_with(Arg::new("out-parent"), &["extract"])
				.long("out-parent")
				.value_name("PATH")
				.value_parser(clap::value_parser!(PathBuf))
				.help(english::text("cli.out_parent")),
			only_with(Arg::new("no-open"), &["extract"])
				.long("no-open")
				.action(ArgAction::SetTrue)
				.help("Do not open the output folder after extraction"),
			only_with(Arg::new("workshop-title"), &["extract"])
				.long("workshop-title")
				.action(ArgAction::SetTrue)
				.help(english::text("cli.workshop_title")),
			only_with(Arg::new("workshop-metadata"), &["extract"])
				.long("workshop-metadata")
				.action(ArgAction::SetTrue)
				.help(english::text("cli.workshop_metadata")),
			Arg::new("metadata-name")
				.long("metadata-name")
				.value_name("NAME")
				.value_parser(metadata_name)
				.requires("workshop-metadata")
				.help(english::text("cli.metadata_name")),
			Arg::new("workshop-id")
				.long("workshop-id")
				.value_name("ID")
				.value_parser(clap::value_parser!(u64).range(1..))
				.requires("workshop")
				.help(english::text("cli.workshop_id")),
			only_with(Arg::new("title"), &["pack", "publish"])
				.long("title")
				.value_name("TITLE")
				.help(english::text("cli.title")),
			only_with(Arg::new("type"), &["pack", "publish"])
				.long("type")
				.value_name("TYPE")
				.value_parser(PossibleValuesParser::new(ADDON_TYPES))
				.ignore_case(true)
				.help(english::text("cli.type")),
			only_with(Arg::new("tags"), &["pack", "publish"])
				.long("tags")
				.value_name("TAGS")
				.value_parser(PossibleValuesParser::new(ADDON_TAGS))
				.ignore_case(true)
				.value_delimiter(',')
				.help(english::text("cli.tags")),
			only_with(Arg::new("update"), &["publish"])
				.long("update")
				.value_name("ID")
				.value_parser(clap::value_parser!(u64).range(1..))
				.help(english::text("cli.update")),
			only_with(Arg::new("icon"), &["publish"])
				.long("icon")
				.value_name("FILE")
				.value_parser(clap::value_parser!(PathBuf))
				.help(english::text("cli.icon")),
			only_with(Arg::new("description-file"), &["publish"])
				.long("description-file")
				.value_name("FILE")
				.value_parser(clap::value_parser!(PathBuf))
				.help(english::text("cli.description_file")),
			only_with(Arg::new("changes"), &["publish"])
				.long("changes")
				.value_name("TEXT")
				.help(english::text("cli.changes")),
		])
		.group(ArgGroup::new("action").args(ACTIONS).required(true))
		.group(ArgGroup::new("workshop").args(["workshop-title", "workshop-metadata"]).multiple(true))
}

pub(super) fn stdin() -> Option<ExitCode> {
	if !*CLI_MODE {
		return None;
	}

	#[cfg(target_os = "windows")]
	attach_console();

	let matches = command().get_matches();
	crate::logging::init(crate::logging::Mode::Cli);

	let result = if let Some(path) = matches.get_one::<PathBuf>("extract") {
		extract(path, &matches)
	} else if let Some(folder) = matches.get_one::<PathBuf>("pack") {
		pack(folder, &matches)
	} else if let Some(path) = matches.get_one::<PathBuf>("list") {
		list(path)
	} else if let Some(path) = matches.get_one::<PathBuf>("info") {
		info(path)
	} else if let Some(folder) = matches.get_one::<PathBuf>("publish") {
		publish(folder, &matches)
	} else {
		unreachable!("clap requires one action")
	};

	let exit_code = match result {
		Ok(()) => {
			println!("Finished");
			ExitCode::SUCCESS
		}
		Err(message) => {
			notify(Level::Error, message);
			print_log_location();
			ExitCode::FAILURE
		}
	};
	crate::logging::flush();
	Some(exit_code)
}

/// Prints a message for the user on standard error and records it in the log.
fn notify(level: Level, message: String) {
	std::eprintln!("{message}");
	crate::logging::write(level, message);
}

pub(crate) fn print_log_location() {
	if let Some(path) = crate::logging::current() {
		std::eprintln!("{}", english::format("cli.log", &[("path", &path.display().to_string())]));
	}
}

/// Prints a result on standard output and records it in the log.
fn print_result(line: String) -> Result<(), String> {
	println!("{line}");
	write_output([line])
}

fn open_failure(path: &Path, error: GMAError) -> String {
	format!("Failed to open archive \"{}\": {}", path.display(), failure_details(error))
}

fn extract(extract_path: &Path, matches: &ArgMatches) -> Result<(), String> {
	let mut gma = GMAFile::open(extract_path).map_err(|error| open_failure(extract_path, error))?;
	let dest = match (matches.get_one::<PathBuf>("out"), matches.get_one::<PathBuf>("out-parent")) {
		(Some(out), _) => ExtractDestination::Directory(out.clone()),
		(None, Some(parent)) => ExtractDestination::NamedDirectory(parent.clone()),
		(None, None) => ExtractDestination::Temp,
	};
	let mut options = ExtractOptions {
		workshop_title: matches.get_flag("workshop-title"),
		metadata_name: matches
			.get_flag("workshop-metadata")
			.then(|| matches.get_one::<String>("metadata-name").cloned().unwrap_or_else(|| DEFAULT_METADATA_FILE_NAME.to_owned())),
		icon: false,
		workshop: None,
	};
	let transaction = transaction!();
	let workshop_id = matches.get_one::<u64>("workshop-id").map(|id| PublishedFileId(*id));
	if let Some(id) = workshop_id {
		gma.set_ws_id(id);
	}
	crate::steam::item_info::attach_cli(&mut options, workshop_id.or_else(|| gma.inferred_ws_id()), workshop_id.is_none(), &transaction);
	// Handle opening here so an opener failure also reaches the caller's exit status.
	let output = gma
		.extract(dest, &transaction, false, true, &options)
		.map_err(|error| format!("Failed to extract archive \"{}\": {}", extract_path.display(), failure_details(error)))?;
	print_result(format!("Extracted to \"{}\"", output.path.display()))?;
	if let Some(metadata) = &output.metadata {
		print_result(english::format("cli.metadata_saved", &[("path", &metadata.display().to_string())]))?;
	}
	let output = output.path;
	if !matches.get_flag("no-open") && app_data!().settings.read().open_folder_after_extract {
		if let Err(error) = crate::path::open_with_default_app(&output) {
			return Err(format!(
				"Extraction succeeded, but failed to open output folder \"{}\": {}. Open it manually or use --no-open.",
				output.display(),
				error
			));
		}
	}
	Ok(())
}

fn read_archive(path: &Path) -> Result<GMAFile, String> {
	let mut gma = GMAFile::open(path).map_err(|error| open_failure(path, error))?;
	gma.entries().map_err(|error| open_failure(path, error))?;
	Ok(gma)
}

fn write_output(lines: impl IntoIterator<Item = String>) -> Result<(), String> {
	let mut stdout = std::io::stdout().lock();
	lines
		.into_iter()
		.try_for_each(|line| writeln!(stdout, "{line}"))
		.and_then(|()| stdout.flush())
		.map_err(|error| english::format("cli.output_failed", &[("error", &error.to_string())]))
}

fn list(path: &Path) -> Result<(), String> {
	let gma = read_archive(path)?;
	let mut entries: Vec<_> = gma.entries.as_ref().expect("entries were read").values().collect();
	entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
	write_output(entries.into_iter().map(|entry| format!("{:>12}  {}", entry.size, entry.path)))
}

fn info(path: &Path) -> Result<(), String> {
	let gma = read_archive(path)?;
	let entries = gma.entries.as_ref().expect("entries were read");
	let line = |key: &str, value: &str| english::format(key, &[("value", value)]);
	let mut lines = Vec::new();
	if let Some(metadata) = &gma.metadata {
		lines.push(line("cli.info_title", metadata.title()));
		if let Some(addon_type) = metadata.addon_type().filter(|addon_type| !addon_type.is_empty()) {
			lines.push(line("cli.info_type", addon_type));
		}
		if let Some(tags) = metadata.tags().filter(|tags| !tags.is_empty()) {
			lines.push(line("cli.info_tags", &tags.join(", ")));
		}
	}
	if let Some(id) = gma.id {
		lines.push(line("cli.info_workshop_id", &id.0.to_string()));
	}
	lines.push(line("cli.info_files", &entries.len().to_string()));
	lines.push(line("cli.info_size", &gma.size.to_string()));
	write_output(lines)
}

#[derive(Default, serde::Deserialize)]
struct AddonJson {
	title: Option<String>,
	#[serde(rename = "type")]
	addon_type: Option<String>,
	#[serde(default)]
	tags: Vec<String>,
	#[serde(default)]
	ignore: Vec<String>,
}

struct AddonDetails {
	folder: PathBuf,
	title: String,
	addon_type: String,
	tags: Vec<String>,
	ignore: Vec<String>,
}

fn canonical(values: &[&'static str], value: &str) -> Option<String> {
	values.iter().find(|candidate| candidate.eq_ignore_ascii_case(value.trim())).map(|candidate| (*candidate).to_owned())
}

/// Reads an addon folder the way gmad does: addon.json supplies the details, and arguments override them.
fn addon_details(folder: &Path, matches: &ArgMatches) -> Result<AddonDetails, String> {
	let folder = dunce::canonicalize(folder).map_err(|error| IoError::new("open addon folder", folder, error).to_string())?;
	let json_path = folder.join("addon.json");
	let json = match std::fs::read_to_string(&json_path) {
		Ok(text) => serde_json::from_str::<AddonJson>(text.strip_prefix('\u{feff}').unwrap_or(&text))
			.map_err(|error| english::format("cli.addon_json_invalid", &[("path", &json_path.display().to_string()), ("error", &error.to_string())]))?,
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => AddonJson::default(),
		Err(error) => return Err(IoError::new("read addon.json", &json_path, error).to_string()),
	};

	let title = matches
		.get_one::<String>("title")
		.cloned()
		.or(json.title)
		.filter(|title| !title.trim().is_empty())
		.ok_or_else(|| english::text("cli.title_missing").to_owned())?;
	let addon_type = matches
		.get_one::<String>("type")
		.cloned()
		.or(json.addon_type)
		.ok_or_else(|| english::text("cli.type_missing").to_owned())?;
	let addon_type = canonical(&ADDON_TYPES, &addon_type)
		.ok_or_else(|| english::format("cli.type_invalid", &[("value", &addon_type), ("values", &ADDON_TYPES.join(", "))]))?;
	let tags = match matches.get_many::<String>("tags") {
		Some(tags) => tags.cloned().collect(),
		None => json.tags,
	};
	let mut canonical_tags = Vec::with_capacity(tags.len());
	for tag in tags {
		let tag = canonical(&ADDON_TAGS, &tag).ok_or_else(|| english::format("cli.tag_invalid", &[("value", &tag), ("values", &ADDON_TAGS.join(", "))]))?;
		if !canonical_tags.contains(&tag) {
			canonical_tags.push(tag);
		}
	}
	if canonical_tags.len() > MAX_TAGS {
		return Err(english::text("cli.too_many_tags").to_owned());
	}

	Ok(AddonDetails { folder, title, addon_type, tags: canonical_tags, ignore: json.ignore })
}

fn pack(folder: &Path, matches: &ArgMatches) -> Result<(), String> {
	let AddonDetails { folder, title, addon_type, tags, ignore: addon_ignore } = addon_details(folder, matches)?;
	let destination = match matches.get_one::<PathBuf>("out") {
		Some(out) => std::path::absolute(out).map_err(|error| IoError::new("resolve package destination", out, error).to_string())?,
		None => match (folder.parent(), folder.file_name()) {
			(Some(parent), Some(name)) => parent.join(format!("{}.gma", name.to_string_lossy())),
			_ => return Err(english::message("ERR_PACKAGE_DESTINATION")),
		},
	};
	let mut ignore = app_data!().settings.read().ignore_globs.clone();
	ignore.extend(addon_ignore);

	let transaction = crate::transactions::new_publish();
	let request = PackageRequest { content_path: folder, destination, title, addon_type, tags };
	match package_archive(request, ignore, &transaction) {
		Ok(path) => {
			transaction.finished(());
			print_result(english::format("cli.packed", &[("path", &path.display().to_string())]))
		}
		Err(error) => {
			let error = error.to_string();
			transaction.error(error.clone(), ());
			Err(english::message(&error))
		}
	}
}

fn connect_steam() -> Result<(), String> {
	notify(Level::Info, english::text("cli.connecting").to_owned());
	std::sync::LazyLock::force(&crate::STEAMWORKS);
	let deadline = Instant::now() + STEAM_CONNECT_TIMEOUT;
	while !steam!().connected() {
		if Instant::now() >= deadline {
			return Err(english::text("cli.steam_unavailable").to_owned());
		}
		sleep_ms!(100);
	}
	Ok(())
}

fn publish(folder: &Path, matches: &ArgMatches) -> Result<(), String> {
	let AddonDetails { folder, title, addon_type, tags, ignore } = addon_details(folder, matches)?;
	if tags.is_empty() {
		return Err(english::text("cli.tags_missing").to_owned());
	}
	let description = match matches.get_one::<PathBuf>("description-file") {
		Some(path) => {
			let text = std::fs::read_to_string(path).map_err(|error| IoError::new("read description", path, error).to_string())?;
			Some(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
		}
		None => None,
	};
	let request = PublishRequest {
		content_path_src: folder,
		icon_path: matches.get_one::<PathBuf>("icon").cloned(),
		title,
		description,
		tags,
		addon_type,
		upscale: false,
		update_id: matches.get_one::<u64>("update").map(|id| PublishedFileId(*id)),
		changes: matches.get_one::<String>("changes").cloned(),
		gma_name: None,
		ignore,
	};

	connect_steam()?;
	notify(Level::Info, english::text("cli.publishing").to_owned());
	let transaction = crate::transactions::new_publish();
	match run_publish(request, &transaction) {
		PublishOutcome::Published { id, legal_agreement, job, warnings, settings_error } => {
			for warning in warnings.iter().chain(&settings_error) {
				notify(Level::Warn, english::message(warning));
			}
			transaction.finished(job.finish(&transaction, "published", warnings));
			if legal_agreement {
				notify(Level::Warn, english::text("cli.legal_agreement").to_owned());
			}
			print_result(english::format("cli.published", &[("id", &id.0.to_string())]))
		}
		PublishOutcome::Cancelled => {
			transaction.cancelled();
			Err(english::message("ERR_CANCELLED"))
		}
		PublishOutcome::Unknown => Err(english::message("ERR_PUBLISH_OUTCOME_UNKNOWN")),
		PublishOutcome::Failed(error, result) => {
			transaction.error(error.clone(), result);
			Err(english::message(&error))
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use clap::error::ErrorKind;

	fn parse(args: &[&str]) -> Result<clap::ArgMatches, ErrorKind> {
		command().try_get_matches_from(["nwmpublisher", "--extract", "addon.gma"].iter().chain(args)).map_err(|error| error.kind())
	}

	fn parse_only(args: &[&str]) -> Result<clap::ArgMatches, ErrorKind> {
		command().try_get_matches_from(["nwmpublisher"].iter().chain(args)).map_err(|error| error.kind())
	}

	#[test]
	fn workshop_arguments_validate_combinations() {
		let matches = parse(&[]).unwrap();
		assert!(!matches.get_flag("workshop-title") && !matches.get_flag("workshop-metadata"));
		let matches = parse(&["--out-parent", "addons", "--workshop-title", "--workshop-metadata", "--metadata-name", "Info.txt", "--workshop-id", "123"]).unwrap();
		assert_eq!(matches.get_one::<String>("metadata-name").unwrap(), "Info.txt");
		assert_eq!(*matches.get_one::<u64>("workshop-id").unwrap(), 123);
		assert!(parse(&["--out", "addon", "--workshop-metadata", "--workshop-id", "1"]).is_ok());
		assert_eq!(parse(&["--out", "a", "--out-parent", "b"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert_eq!(parse(&["--out", "a", "--workshop-title"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert_eq!(parse(&["--metadata-name", "info.txt"]).unwrap_err(), ErrorKind::MissingRequiredArgument);
		assert_eq!(parse(&["--workshop-title", "--metadata-name", "info.txt"]).unwrap_err(), ErrorKind::MissingRequiredArgument);
		assert_eq!(parse(&["--workshop-id", "123"]).unwrap_err(), ErrorKind::MissingRequiredArgument);
		for name in ["info.md", "../info.txt", "CON.txt"] {
			assert_eq!(parse(&["--workshop-metadata", "--metadata-name", name]).unwrap_err(), ErrorKind::ValueValidation, "{name}");
		}
		for id in ["0", "-1", "abc"] {
			assert!(matches!(parse(&["--workshop-title", "--workshop-id", id]).unwrap_err(), ErrorKind::ValueValidation | ErrorKind::InvalidValue | ErrorKind::UnknownArgument), "{id}");
		}
	}

	#[test]
	fn actions_are_required_exclusive_and_own_their_options() {
		assert_eq!(parse_only(&["--no-open"]).unwrap_err(), ErrorKind::MissingRequiredArgument);
		assert_eq!(parse_only(&["--list", "a.gma", "--info", "a.gma"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert_eq!(parse(&["--pack", "addon"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert!(parse_only(&["--list", "a.gma"]).is_ok());
		assert!(parse_only(&["--pack", "addon", "--out", "addon.gma", "--title", "Addon"]).is_ok());
		assert_eq!(parse_only(&["--list", "a.gma", "--out", "b"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert_eq!(parse_only(&["--publish", "addon", "--out", "b"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert_eq!(parse_only(&["--pack", "addon", "--no-open"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert_eq!(parse_only(&["--pack", "addon", "--workshop-metadata"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert_eq!(parse(&["--title", "Addon"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert_eq!(parse_only(&["--info", "a.gma", "--tags", "fun"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert_eq!(parse_only(&["--pack", "addon", "--update", "1"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert_eq!(parse_only(&["--pack", "addon", "--icon", "icon.png"]).unwrap_err(), ErrorKind::ArgumentConflict);
		assert_eq!(parse_only(&["--changes", "Fixes"]).unwrap_err(), ErrorKind::MissingRequiredArgument);
		let matches = parse_only(&["--publish", "addon", "--update", "123", "--icon", "icon.png", "--changes", "Fixes", "--description-file", "README.txt"]).unwrap();
		assert_eq!(*matches.get_one::<u64>("update").unwrap(), 123);
		assert_eq!(matches.get_one::<String>("changes").unwrap(), "Fixes");
		assert_eq!(parse_only(&["--publish", "addon", "--update", "0"]).unwrap_err(), ErrorKind::ValueValidation);
	}

	#[test]
	fn addon_types_and_tags_are_validated_ignoring_case() {
		let matches = parse_only(&["--pack", "addon", "--type", "servercontent", "--tags", "Fun,build"]).unwrap();
		assert_eq!(matches.get_one::<String>("type").unwrap(), "servercontent");
		assert_eq!(matches.get_many::<String>("tags").unwrap().collect::<Vec<_>>(), ["Fun", "build"]);
		assert_eq!(parse_only(&["--pack", "addon", "--type", "weapons"]).unwrap_err(), ErrorKind::InvalidValue);
		assert_eq!(parse_only(&["--pack", "addon", "--tags", "fun,cars"]).unwrap_err(), ErrorKind::InvalidValue);
		assert_eq!(canonical(&ADDON_TYPES, "servercontent").as_deref(), Some("ServerContent"));
		assert_eq!(canonical(&ADDON_TAGS, " Fun ").as_deref(), Some("fun"));
		assert_eq!(canonical(&ADDON_TAGS, "cars"), None);
	}

	#[test]
	fn addon_details_combine_addon_json_and_arguments() {
		let root = tempfile::tempdir().unwrap();
		let folder = root.path().join("addon");
		std::fs::create_dir(&folder).unwrap();
		let details = |args: &[&str]| addon_details(&folder, &parse_only(&[&["--pack", "addon"], args].concat()).unwrap());

		assert_eq!(details(&[]).err().unwrap(), english::text("cli.title_missing"));
		assert_eq!(details(&["--title", "Addon"]).err().unwrap(), english::text("cli.type_missing"));
		let from_args = details(&["--title", "Addon", "--type", "TOOL"]).unwrap();
		assert_eq!((from_args.title.as_str(), from_args.addon_type.as_str(), from_args.tags.len()), ("Addon", "tool", 0));

		std::fs::write(folder.join("addon.json"), "\u{feff}{\"title\":\"From JSON\",\"type\":\"servercontent\",\"tags\":[\"Fun\",\"fun\",\"build\"],\"ignore\":[\"*.psd\"]}").unwrap();
		let from_json = details(&[]).unwrap();
		assert_eq!(from_json.folder, dunce::canonicalize(&folder).unwrap());
		assert_eq!((from_json.title.as_str(), from_json.addon_type.as_str()), ("From JSON", "ServerContent"));
		assert_eq!((from_json.tags, from_json.ignore), (vec!["fun".to_owned(), "build".to_owned()], vec!["*.psd".to_owned()]));
		let overridden = details(&["--title", "Override", "--type", "map", "--tags", "scenic"]).unwrap();
		assert_eq!((overridden.title.as_str(), overridden.addon_type.as_str(), overridden.tags), ("Override", "map", vec!["scenic".to_owned()]));
		assert!(details(&["--tags", "fun,build,water,comic"]).err().unwrap().contains(english::text("cli.too_many_tags")));

		std::fs::write(folder.join("addon.json"), "{\"title\":\"Addon\",\"type\":\"weapons\"}").unwrap();
		assert!(details(&[]).err().unwrap().contains("weapons"));
		std::fs::write(folder.join("addon.json"), "{\"title\":\"Addon\",\"type\":\"tool\",\"tags\":[\"cars\"]}").unwrap();
		assert!(details(&[]).err().unwrap().contains("cars"));
		std::fs::write(folder.join("addon.json"), "{").unwrap();
		assert!(details(&[]).err().unwrap().contains("addon.json"));
	}
}
