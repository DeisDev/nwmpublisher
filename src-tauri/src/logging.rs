use std::{
	fs::{self, File, OpenOptions},
	io,
	io::Write,
	panic::PanicHookInfo,
	path::{Path, PathBuf},
	sync::atomic::{AtomicBool, Ordering},
	time::Duration,
};

use chrono::{DateTime, Local, NaiveDateTime};
use crossbeam::channel::{Receiver, Sender};
use parking_lot::Mutex;

use crate::{app_data, ignore};

const OLDER_LOGS_KEPT: usize = 3;
const FILE_TIME_FORMAT: &str = "%Y-%m-%d_%H-%M-%S";
const FILE_TIME_LENGTH: usize = "0000-00-00_00-00-00".len();
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(target_os = "windows")]
const FILE_SHARE_READ: u32 = 0x1;
#[cfg(target_os = "windows")]
const FILE_SHARE_WRITE: u32 = 0x2;

#[derive(Clone, Copy)]
pub enum Level {
	Info,
	Warn,
	Error,
}
impl Level {
	fn name(self) -> &'static str {
		match self {
			Level::Info => "INFO",
			Level::Warn => "WARN",
			Level::Error => "ERROR",
		}
	}
}

#[derive(Clone, Copy)]
pub enum Mode {
	Desktop,
	Cli,
}
impl Mode {
	fn name(self) -> &'static str {
		match self {
			Mode::Desktop => "desktop",
			Mode::Cli => "cli",
		}
	}
}

enum Message {
	Line(DateTime<Local>, Level, String),
	Relocate(PathBuf, Sender<Result<(), String>>),
	Flush(Sender<()>),
}

// Messages sent before init() wait here, so startup lines reach the log once its folder is known.
static CHANNEL: std::sync::LazyLock<(Sender<Message>, Receiver<Message>)> = std::sync::LazyLock::new(crossbeam::channel::unbounded);
static STARTED: AtomicBool = AtomicBool::new(false);
static CURRENT: std::sync::LazyLock<Mutex<Option<PathBuf>>> = std::sync::LazyLock::new(|| Mutex::new(None));

/// Records a line in the log. The desktop app also prints it; the CLI keeps its terminal for results and messages it prints itself.
pub fn write(level: Level, message: String) {
	if !*crate::cli::CLI_MODE {
		match level {
			Level::Info => std::println!("{message}"),
			Level::Warn | Level::Error => std::eprintln!("{message}"),
		}
	}
	ignore! { CHANNEL.0.send(Message::Line(Local::now(), level, message)) };
}

#[macro_export]
macro_rules! println {
	($($arg:tt)*) => {
		$crate::logging::write($crate::logging::Level::Info, format!($($arg)*))
	};
}

#[macro_export]
macro_rules! wprintln {
	($($arg:tt)*) => {
		$crate::logging::write($crate::logging::Level::Warn, format!($($arg)*))
	};
}

#[macro_export]
macro_rules! eprintln {
	($($arg:tt)*) => {
		$crate::logging::write($crate::logging::Level::Error, format!($($arg)*))
	};
}

struct LogFile {
	stem: String,
	path: PathBuf,
	file: File,
}
impl LogFile {
	fn create(mode: Mode, directory: &Path, started: DateTime<Local>) -> io::Result<LogFile> {
		let stem = format!("{}_{}", mode.name(), started.format(FILE_TIME_FORMAT));
		let (path, mut file) = create_unique(directory, &stem)?;
		file.write_all(header(mode, started).as_bytes())?;
		Ok(LogFile { stem, path, file })
	}

	fn write(&mut self, time: DateTime<Local>, level: Level, message: &str) {
		let line = format!("{} {:<5} {}\n", time.format("%Y-%m-%d %H:%M:%S%.3f"), level.name(), message.trim_end());
		if let Err(error) = self.file.write_all(line.as_bytes()) {
			std::eprintln!("Failed to write to log {}: {error}", self.path.display());
		}
	}

	fn relocate(&mut self, directory: &Path) -> io::Result<()> {
		let (path, mut file) = create_unique(directory, &self.stem)?;
		if let Err(error) = File::open(&self.path).and_then(|mut current| io::copy(&mut current, &mut file)) {
			drop(file);
			if let Err(cleanup) = fs::remove_file(&path) {
				wprintln!("Could not remove the incomplete log copy {}: {cleanup}", path.display());
			}
			return Err(error);
		}
		let previous = std::mem::replace(&mut self.path, path);
		self.file = file;
		if let Err(error) = fs::remove_file(&previous) {
			wprintln!("Could not remove the log from its previous folder {}: {error}", previous.display());
		}
		Ok(())
	}
}

fn options() -> OpenOptions {
	let mut options = OpenOptions::new();
	options.append(true);
	// Leaving out delete sharing stops other running instances from pruning a log that is still being written.
	#[cfg(target_os = "windows")]
	std::os::windows::fs::OpenOptionsExt::share_mode(&mut options, FILE_SHARE_READ | FILE_SHARE_WRITE);
	options
}

fn create_unique(directory: &Path, stem: &str) -> io::Result<(PathBuf, File)> {
	fs::create_dir_all(directory)?;
	let mut attempt = 1;
	loop {
		let name = if attempt == 1 { format!("{stem}.log") } else { format!("{stem}_{attempt}.log") };
		let path = directory.join(name);
		match options().create_new(true).open(&path) {
			Ok(file) => return Ok((path, file)),
			Err(error) if error.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
			Err(error) => return Err(error),
		}
	}
}

fn header(mode: Mode, started: DateTime<Local>) -> String {
	let os = os_info::get();
	let mut lines = vec![
		format!("nwmpublisher {} ({}, {})", env!("CARGO_PKG_VERSION"), mode.name(), std::env::consts::ARCH),
		format!("Started: {}", started.format("%Y-%m-%d %H:%M:%S %:z")),
		format!("OS: {os}, {}", os.architecture().unwrap_or("unknown architecture")),
	];
	match mode {
		Mode::Desktop => lines.push(match tauri::webview_version() {
			Ok(version) => format!("WebView: {version}"),
			Err(error) => format!("WebView: unavailable ({error})"),
		}),
		Mode::Cli => lines.push(format!("Command line: {}", command_line())),
	}
	lines.join("\n") + "\n\n"
}

fn command_line() -> String {
	std::env::args_os()
		.map(|arg| {
			let arg = arg.to_string_lossy();
			if arg.is_empty() || arg.contains(char::is_whitespace) { format!("\"{arg}\"") } else { arg.into_owned() }
		})
		.collect::<Vec<_>>()
		.join(" ")
}

/// Sorts this mode's logs by start time; other files in the folder are never matched.
fn log_order(mode: Mode, file_name: &str) -> Option<(NaiveDateTime, u32)> {
	let rest = file_name.strip_prefix(mode.name())?.strip_prefix('_')?.strip_suffix(".log")?;
	let (time, suffix) = rest.split_at_checked(FILE_TIME_LENGTH)?;
	let attempt = match suffix {
		"" => 1,
		suffix => suffix.strip_prefix('_')?.parse().ok()?,
	};
	Some((NaiveDateTime::parse_from_str(time, FILE_TIME_FORMAT).ok()?, attempt))
}

fn prune(directory: &Path, mode: Mode, current: &Path) {
	let entries = match fs::read_dir(directory) {
		Ok(entries) => entries,
		Err(error) => return wprintln!("Could not list old logs in {}: {error}", directory.display()),
	};
	let mut logs: Vec<_> = entries
		.filter_map(|entry| {
			let entry = entry.ok()?;
			Some((log_order(mode, entry.file_name().to_str()?)?, entry.path()))
		})
		.filter(|(_, path)| path != current)
		.collect();
	logs.sort_unstable();
	let excess = logs.len().saturating_sub(OLDER_LOGS_KEPT);
	for (_, path) in logs.into_iter().take(excess) {
		if let Err(error) = fs::remove_file(&path) {
			wprintln!("Could not remove old log {}: {error}", path.display());
		}
	}
}

fn same_directory(a: &Path, b: &Path) -> bool {
	match (dunce::canonicalize(a), dunce::canonicalize(b)) {
		(Ok(a), Ok(b)) => a == b,
		_ => a == b,
	}
}

fn move_log(log: &mut Option<LogFile>, mode: Mode, started: DateTime<Local>, directory: &Path) -> io::Result<()> {
	match log {
		Some(log) if log.path.parent().is_some_and(|current| same_directory(current, directory)) => return Ok(()),
		Some(log) => log.relocate(directory)?,
		None => *log = Some(LogFile::create(mode, directory, started)?),
	}
	let path = log.as_ref().map(|log| log.path.clone()).expect("log was just moved or created");
	println!("Log moved to {}", path.display());
	prune(directory, mode, &path);
	*CURRENT.lock() = Some(path);
	Ok(())
}

fn run(mut log: Option<LogFile>, mode: Mode, started: DateTime<Local>, receiver: Receiver<Message>) {
	for message in receiver {
		match message {
			Message::Line(time, level, text) => {
				if let Some(log) = &mut log {
					log.write(time, level, &text);
				}
			}
			Message::Relocate(directory, reply) => {
				let result = move_log(&mut log, mode, started, &directory).map_err(|error| error.to_string());
				if let Err(error) = &result {
					eprintln!("Could not move the log to {}: {error}", directory.display());
				}
				reply.send(result).ok();
			}
			Message::Flush(reply) => {
				reply.send(()).ok();
			}
		}
	}
}

pub fn init(mode: Mode) {
	let started = Local::now();
	let directory = app_data!().logs_dir();
	let log = match LogFile::create(mode, &directory, started) {
		Ok(log) => {
			prune(&directory, mode, &log.path);
			*CURRENT.lock() = Some(log.path.clone());
			Some(log)
		}
		Err(error) => {
			std::eprintln!("Could not create a log file in {}: {error}", directory.display());
			None
		}
	};
	let receiver = CHANNEL.1.clone();
	std::thread::spawn(move || run(log, mode, started, receiver));
	STARTED.store(true, Ordering::Release);
}

pub fn current() -> Option<PathBuf> {
	CURRENT.lock().clone()
}

/// Moves the current log into `directory` and keeps writing there.
pub fn relocate(directory: PathBuf) -> Result<(), String> {
	if !STARTED.load(Ordering::Acquire) {
		return Ok(());
	}
	let (reply, result) = crossbeam::channel::bounded(1);
	ignore! { CHANNEL.0.send(Message::Relocate(directory, reply)) };
	result.recv_timeout(REPLY_TIMEOUT).unwrap_or_else(|error| {
		eprintln!("Timed out moving the log: {error}");
		Err(error.to_string())
	})
}

/// Waits until every line sent so far is written, for a bounded time so a stalled disk cannot hang exit.
pub fn flush() {
	if !STARTED.load(Ordering::Acquire) {
		return;
	}
	let (reply, done) = crossbeam::channel::bounded(1);
	ignore! { CHANNEL.0.send(Message::Flush(reply)) };
	if let Err(error) = done.recv_timeout(REPLY_TIMEOUT) {
		std::eprintln!("Timed out writing the log: {error}");
	}
}

pub fn panic(panic: &PanicHookInfo<'_>) {
	let backtrace = backtrace::Backtrace::new();
	eprintln!("PANIC: {panic}\n{backtrace:#?}");
	flush();
	if *crate::cli::CLI_MODE {
		std::eprintln!("{panic}");
		crate::cli::print_log_location();
	}
}

#[tauri::command]
pub async fn open_logs() -> Result<(), String> {
	let directory = current().and_then(|path| path.parent().map(Path::to_path_buf)).unwrap_or_else(|| app_data!().logs_dir());
	tauri::async_runtime::spawn_blocking(move || {
		crate::path::open_with_default_app(&directory).map_err(|error| {
			eprintln!("Could not open the logs folder {}: {error}", directory.display());
			format!("ERR_OPEN_LOGS:{}: {error}", directory.display())
		})
	})
	.await
	.map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
	use super::*;

	fn local(time: &str) -> DateTime<Local> {
		NaiveDateTime::parse_from_str(time, FILE_TIME_FORMAT).unwrap().and_local_timezone(Local).earliest().unwrap()
	}

	#[test]
	fn logs_are_named_by_mode_and_start_time_without_overwriting() {
		let folder = tempfile::tempdir().unwrap();
		let started = local("2026-10-19_14-32-05");
		let first = LogFile::create(Mode::Desktop, folder.path(), started).unwrap();
		let second = LogFile::create(Mode::Desktop, folder.path(), started).unwrap();
		let cli = LogFile::create(Mode::Cli, folder.path(), started).unwrap();
		assert_eq!(first.path.file_name().unwrap(), "desktop_2026-10-19_14-32-05.log");
		assert_eq!(second.path.file_name().unwrap(), "desktop_2026-10-19_14-32-05_2.log");
		assert_eq!(cli.path.file_name().unwrap(), "cli_2026-10-19_14-32-05.log");
		let text = fs::read_to_string(&first.path).unwrap();
		assert!(text.starts_with(&format!("nwmpublisher {} (desktop,", env!("CARGO_PKG_VERSION"))));
		assert!(text.contains("\nOS: "));
		assert!(fs::read_to_string(&cli.path).unwrap().contains("\nCommand line: "));
		assert!(log_order(Mode::Desktop, "desktop_2026-10-19_14-32-05.log") < log_order(Mode::Desktop, "desktop_2026-10-19_14-32-05_2.log"));
		for name in ["desktop_notes.log", "desktop_2026-10-19_14-32-05.txt", "cli_2026-10-19_14-32-05.log", "desktop_2026-10-19_14-32-05_x.log"] {
			assert_eq!(log_order(Mode::Desktop, name), None, "{name}");
		}
	}

	#[test]
	fn pruning_keeps_the_current_log_and_three_older_ones_per_mode() {
		let folder = tempfile::tempdir().unwrap();
		let names = [
			"desktop_2026-10-14_09-00-00.log",
			"desktop_2026-10-15_09-00-00.log",
			"desktop_2026-10-16_09-00-00.log",
			"desktop_2026-10-17_09-00-00.log",
			"desktop_2026-10-17_09-00-00_2.log",
			"cli_2026-10-01_09-00-00.log",
			"desktop_notes.log",
			"readme.txt",
		];
		for name in names {
			fs::write(folder.path().join(name), "").unwrap();
		}
		let current = LogFile::create(Mode::Desktop, folder.path(), local("2026-10-19_14-32-05")).unwrap();
		prune(folder.path(), Mode::Desktop, &current.path);
		let mut remaining: Vec<_> = fs::read_dir(folder.path()).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
		remaining.sort();
		assert_eq!(
			remaining,
			[
				"cli_2026-10-01_09-00-00.log",
				"desktop_2026-10-16_09-00-00.log",
				"desktop_2026-10-17_09-00-00.log",
				"desktop_2026-10-17_09-00-00_2.log",
				"desktop_2026-10-19_14-32-05.log",
				"desktop_notes.log",
				"readme.txt",
			]
		);
	}

	#[test]
	fn moving_a_log_keeps_its_name_and_contents_and_continues_writing() {
		let folder = tempfile::tempdir().unwrap();
		let started = local("2026-10-19_14-32-05");
		let mut log = Some(LogFile::create(Mode::Desktop, &folder.path().join("old"), started).unwrap());
		log.as_mut().unwrap().write(started, Level::Warn, "before\n");
		let old = log.as_ref().unwrap().path.clone();
		let new = folder.path().join("new");
		fs::create_dir(&new).unwrap();
		fs::write(new.join("desktop_2026-10-19_14-32-05.log"), "another instance").unwrap();
		move_log(&mut log, Mode::Desktop, started, &new).unwrap();
		log.as_mut().unwrap().write(started, Level::Error, "after");
		let moved = log.as_ref().unwrap().path.clone();
		assert_eq!(moved, new.join("desktop_2026-10-19_14-32-05_2.log"));
		assert!(!old.exists());
		let text = fs::read_to_string(&moved).unwrap();
		assert!(text.starts_with("nwmpublisher "));
		assert!(text.ends_with(" WARN  before\n2026-10-19 14:32:05.000 ERROR after\n"), "{text}");
		assert_eq!(fs::read_to_string(new.join("desktop_2026-10-19_14-32-05.log")).unwrap(), "another instance");
		move_log(&mut log, Mode::Desktop, started, &new).unwrap();
		assert_eq!(log.as_ref().unwrap().path, moved);
	}
}
