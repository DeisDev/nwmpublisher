use std::{path::PathBuf, process::ExitCode};

use clap::{Arg, ArgAction, ArgGroup, Command};
use steamworks::PublishedFileId;

use crate::{
	gma::{
		filename::{valid_metadata_file_name, DEFAULT_METADATA_FILE_NAME},
		ExtractDestination, ExtractGMAMut, ExtractOptions,
	},
	util::english,
	GMAError, GMAFile,
};

lazy_static! {
	pub static ref CLI_MODE: bool = std::env::args_os().len() > 1;
}

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

// Future publishing arguments must use the GUI's publishing job implementation.
fn command() -> Command {
	Command::new("nwmpublisher")
		.version(env!("CARGO_PKG_VERSION"))
		.author("William Venner <william@venner.io>")
		.about("Extract GMA files")
		.after_help("Run without arguments to launch the GUI.\nExit codes: 0 = success, 1 = operation failed, 2 = invalid arguments.")
		.args([
			Arg::new("extract")
				.short('e')
				.long("extract")
				.value_name("FILE")
				.value_parser(clap::value_parser!(PathBuf))
				.required(true)
				.help("Extract a .GMA file"),
			Arg::new("out")
				.short('o')
				.long("out")
				.value_name("PATH")
				.value_parser(clap::value_parser!(PathBuf))
				.conflicts_with_all(["out-parent", "workshop-title"])
				.help("Extract directly into PATH (default: an addon folder in the configured temporary directory)"),
			Arg::new("out-parent")
				.long("out-parent")
				.value_name("PATH")
				.value_parser(clap::value_parser!(PathBuf))
				.help(english::text("cli.out_parent")),
			Arg::new("no-open")
				.long("no-open")
				.action(ArgAction::SetTrue)
				.help("Do not open the output folder after extraction"),
			Arg::new("workshop-title")
				.long("workshop-title")
				.action(ArgAction::SetTrue)
				.help(english::text("cli.workshop_title")),
			Arg::new("workshop-metadata")
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
		])
		.group(ArgGroup::new("workshop").args(["workshop-title", "workshop-metadata"]).multiple(true))
}

pub(super) fn stdin() -> Option<ExitCode> {
	if !*CLI_MODE {
		return None;
	}

	#[cfg(target_os = "windows")]
	attach_console();

	// Remove the logging::panic() hook.
	let _ = std::panic::take_hook();

	let matches = command().get_matches();

	let extract_path = matches.get_one::<PathBuf>("extract").expect("required extraction path");
	let mut gma = match GMAFile::open(extract_path) {
		Ok(gma) => gma,
		Err(error) => {
			std::eprintln!("Failed to open archive \"{}\": {}", extract_path.display(), failure_details(error));
			return Some(ExitCode::FAILURE);
		}
	};
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
		workshop: None,
	};
	let transaction = transaction!();
	let workshop_id = matches.get_one::<u64>("workshop-id").map(|id| PublishedFileId(*id));
	if let Some(id) = workshop_id {
		gma.set_ws_id(id);
	}
	crate::steam::item_info::attach_cli(&mut options, workshop_id.or_else(|| gma.inferred_ws_id()), workshop_id.is_none(), &transaction);
	// Handle opening here so an opener failure also reaches the caller's exit status.
	let output = match gma.extract(dest, &transaction, false, true, &options) {
		Ok(output) => output,
		Err(error) => {
			std::eprintln!("Failed to extract archive \"{}\": {}", extract_path.display(), failure_details(error));
			return Some(ExitCode::FAILURE);
		}
	};
	std::println!("Extracted to \"{}\"", output.path.display());
	if let Some(metadata) = &output.metadata {
		std::println!("{}", english::format("cli.metadata_saved", &[("path", &metadata.display().to_string())]));
	}
	let output = output.path;
	if !matches.get_flag("no-open") && app_data!().settings.read().open_folder_after_extract {
		if let Err(error) = opener::open(&output) {
			let details = match error {
				opener::OpenError::Io(error) => error.to_string(),
				error => error.to_string(),
			};
			std::eprintln!(
				"Extraction succeeded, but failed to open output folder \"{}\": {}. Open it manually or use --no-open.",
				output.display(),
				details
			);
			return Some(ExitCode::FAILURE);
		}
	}

	Some(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
	use super::*;
	use clap::error::ErrorKind;

	fn parse(args: &[&str]) -> Result<clap::ArgMatches, ErrorKind> {
		command().try_get_matches_from(["nwmpublisher", "--extract", "addon.gma"].iter().chain(args)).map_err(|error| error.kind())
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
}
