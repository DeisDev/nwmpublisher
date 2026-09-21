use std::{path::PathBuf, process::ExitCode};

use crate::{
	gma::{ExtractDestination, ExtractGMAMut},
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

pub(super) fn stdin() -> Option<ExitCode> {
	use clap::{Arg, ArgAction, Command};

	if !*CLI_MODE {
		return None;
	}

	#[cfg(target_os = "windows")]
	attach_console();

	// Remove the logging::panic() hook.
	let _ = std::panic::take_hook();

	// Future publishing arguments must use the GUI's publishing job implementation.
	let matches = Command::new("nwmpublisher")
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
				.help("Extract directly into PATH (default: an addon folder in the configured temporary directory)"),
			Arg::new("no-open")
				.long("no-open")
				.action(ArgAction::SetTrue)
				.help("Do not open the output folder after extraction"),
		])
		.get_matches();

	let extract_path = matches.get_one::<PathBuf>("extract").expect("required extraction path");
	let mut gma = match GMAFile::open(extract_path) {
		Ok(gma) => gma,
		Err(error) => {
			std::eprintln!("Failed to open archive \"{}\": {}", extract_path.display(), failure_details(error));
			return Some(ExitCode::FAILURE);
		}
	};
	let dest = match matches.get_one::<PathBuf>("out") {
		Some(out) => ExtractDestination::Directory(out.clone()),
		None => ExtractDestination::Temp,
	};
	// Handle opening here so an opener failure also reaches the caller's exit status.
	let output = match gma.extract(dest, &transaction!(), false, true) {
		Ok(output) => output,
		Err(error) => {
			std::eprintln!("Failed to extract archive \"{}\": {}", extract_path.display(), failure_details(error));
			return Some(ExitCode::FAILURE);
		}
	};
	std::println!("Extracted to \"{}\"", output.display());
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
