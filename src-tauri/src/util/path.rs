use serde::{de::Visitor, Deserialize, Serialize};
use std::{
	fmt::Debug,
	path::{Path, PathBuf},
};
use tauri_plugin_dialog::DialogExt;

pub fn canonicalize(path: PathBuf) -> PathBuf {
	dunce::canonicalize(path.clone()).unwrap_or(path)
}

#[cfg(not(target_os = "windows"))]
pub fn normalize(path: PathBuf) -> PathBuf {
	canonicalize(path)
}

#[cfg(target_os = "windows")]
pub fn normalize(path: PathBuf) -> PathBuf {
	match dunce::canonicalize(&path) {
		Ok(canonicalized) => PathBuf::from(canonicalized.to_string_lossy().to_string().replace('\\', "/")),
		Err(_) => path,
	}
}

#[derive(Clone)]
pub struct NormalizedPathBuf {
	pub normalized: PathBuf,
	path: PathBuf,
}
impl Default for NormalizedPathBuf {
	fn default() -> Self {
		Self::new()
	}
}

impl NormalizedPathBuf {
	pub fn new() -> NormalizedPathBuf {
		NormalizedPathBuf {
			path: PathBuf::new(),
			normalized: PathBuf::new(),
		}
	}
}
impl AsRef<Path> for NormalizedPathBuf {
	fn as_ref(&self) -> &Path {
		self.path.as_ref()
	}
}
impl PartialEq for NormalizedPathBuf {
	fn eq(&self, other: &Self) -> bool {
		self.path.eq(&other.path)
	}
}
impl Eq for NormalizedPathBuf {}
impl PartialOrd for NormalizedPathBuf {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		Some(self.cmp(other))
	}
}
impl Ord for NormalizedPathBuf {
	fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		self.path.cmp(&other.path)
	}
}
impl std::ops::Deref for NormalizedPathBuf {
	type Target = PathBuf;
	fn deref(&self) -> &Self::Target {
		&self.path
	}
}
impl From<PathBuf> for NormalizedPathBuf {
	fn from(path: PathBuf) -> Self {
		Self {
			path: path.clone(),
			normalized: normalize(path),
		}
	}
}
impl From<&PathBuf> for NormalizedPathBuf {
	fn from(path: &PathBuf) -> Self {
		let path = path.to_owned();
		Self {
			path: path.clone(),
			normalized: normalize(path),
		}
	}
}
impl From<String> for NormalizedPathBuf {
	fn from(path: String) -> Self {
		let path = PathBuf::from(path);
		Self {
			path: path.clone(),
			normalized: normalize(path),
		}
	}
}
impl From<&str> for NormalizedPathBuf {
	fn from(path: &str) -> Self {
		let path = PathBuf::from(path);
		Self {
			path: path.clone(),
			normalized: normalize(path),
		}
	}
}
impl Debug for NormalizedPathBuf {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.path.fmt(f)
	}
}

impl Serialize for NormalizedPathBuf {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		serializer.serialize_str(&self.normalized.to_string_lossy())
	}
}

struct NormalizedPathBufVisitor;
impl<'de> Visitor<'de> for NormalizedPathBufVisitor {
	type Value = String;

	fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
		formatter.write_str("a string")
	}
}
impl<'de> Deserialize<'de> for NormalizedPathBuf {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		Ok(NormalizedPathBuf::from(deserializer.deserialize_string(NormalizedPathBufVisitor)?))
	}
}

#[inline]
pub fn has_extension<P: AsRef<Path>, S: AsRef<str>>(path: P, extension: S) -> bool {
	path.as_ref()
		.extension()
		.map(|x| x.to_str().map(|x| x.eq_ignore_ascii_case(extension.as_ref())).unwrap_or(false))
		.unwrap_or(false)
}

pub fn open<P: AsRef<Path>>(path: P) {
	let path = path.as_ref();
	if let Err(error) = open_with_default_app(path) {
		if *crate::cli::CLI_MODE {
			eprintln!("Failed to open {}: {}", path.display(), error);
			return;
		}
		webview!().window().dialog().message(path.to_string_lossy()).title("File").show(|_| {});
	}
}

#[cfg(not(target_os = "linux"))]
pub fn open_with_default_app(path: &Path) -> std::io::Result<()> {
	opener::open(path).map_err(std::io::Error::other)
}

#[cfg(target_os = "linux")]
pub fn open_with_default_app(path: &Path) -> std::io::Result<()> {
	let status = without_steam_identity(&mut std::process::Command::new("xdg-open"))
		.arg(path)
		.stdin(std::process::Stdio::null())
		.stdout(std::process::Stdio::null())
		.status()?;
	if status.success() {
		Ok(())
	} else {
		Err(std::io::Error::other(format!("xdg-open {status}")))
	}
}

// Steamworks leaves Garry's Mod's app ID in our environment, and clearing it there is unsound on Linux once threads run.
#[cfg(target_os = "linux")]
fn without_steam_identity(command: &mut std::process::Command) -> &mut std::process::Command {
	command.env_remove("SteamAppId").env_remove("SteamGameId")
}

pub fn open_file_location<P: AsRef<Path>>(path: P) {
	let path = dunce::canonicalize(path.as_ref()).unwrap_or_else(|_| path.as_ref().to_path_buf());

	if (|| {
		#[cfg(target_os = "windows")]
		return std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();

		#[cfg(target_os = "macos")]
		return std::process::Command::new("open").arg("-R").arg(&path).spawn();

		#[cfg(target_os = "linux")]
		{
			let path = path.to_string_lossy().into_owned();
			if path.contains(',') || path.contains('"') || path.contains('\\') {
				let new_path = match std::fs::metadata(&path).unwrap().is_dir() {
					true => path,
					false => {
						let mut path2 = PathBuf::from(path);
						path2.pop();
						path2.into_os_string().into_string().unwrap()
					}
				};
				return without_steam_identity(&mut std::process::Command::new("xdg-open")).arg(&new_path).spawn();
			} else {
				if let Ok(fork::Fork::Child) = fork::daemon(false, false) {
					return without_steam_identity(&mut std::process::Command::new("dbus-send"))
						.args([
							"--session",
							"--dest=org.freedesktop.FileManager1",
							"--type=method_call",
							"/org/freedesktop/FileManager1",
							"org.freedesktop.FileManager1.ShowItems",
							format!("array:string:\"file://{path}\"").as_str(),
							"string:\"\"",
						])
						.spawn();
				}
			};
		}

		#[allow(unreachable_code)]
		Err(std::io::Error::other("Unsupported OS"))
	})().is_err() {
		webview!().window().dialog().message(path.display().to_string()).title("File Location").show(|_| {});
	}
}
