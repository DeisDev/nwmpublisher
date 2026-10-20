use std::sync::{atomic::AtomicBool, OnceLock};

use serde::Serialize;
use tauri::{Emitter, WebviewWindow, Wry};

use crate::{GMAFile, WorkshopItem};

pub struct WrappedWebview {
	window: OnceLock<WebviewWindow<Wry>>,
}
impl WrappedWebview {
	pub const fn pending() -> Self {
		Self { window: OnceLock::new() }
	}

	pub fn init(&self, window: WebviewWindow<Wry>) {
		if self.window.set(window).is_err() {
			panic!("webview window initialized twice");
		}
	}

	pub fn emit<D: Serialize + Send + 'static>(&self, event: &'static str, payload: Option<D>) {
		// The CLI never creates a window, so waiting for one would block forever.
		if *crate::cli::CLI_MODE {
			return;
		}
		ignore! { self.window().emit(event, &payload) };
	}

	pub fn window(&self) -> &WebviewWindow<Wry> {
		self.window.wait()
	}
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum Addon {
	Installed(GMAFile),
	Workshop(WorkshopItem),
}
impl Addon {
	#[inline(always)]
	pub fn installed(&self) -> &GMAFile {
		match self {
			Addon::Installed(addon) => addon,
			Addon::Workshop(_) => unreachable!(),
		}
	}
	#[inline(always)]
	pub fn workshop(&self) -> &WorkshopItem {
		match self {
			Addon::Installed(_) => unreachable!(),
			Addon::Workshop(addon) => addon,
		}
	}
}
impl From<Addon> for GMAFile {
	fn from(val: Addon) -> Self {
		match val {
			Addon::Installed(addon) => addon,
			Addon::Workshop(_) => unreachable!(),
		}
	}
}
impl From<Addon> for WorkshopItem {
	fn from(val: Addon) -> Self {
		match val {
			Addon::Installed(_) => unreachable!(),
			Addon::Workshop(addon) => addon,
		}
	}
}
impl From<GMAFile> for Addon {
	fn from(installed: GMAFile) -> Self {
		Addon::Installed(installed)
	}
}
impl From<WorkshopItem> for Addon {
	fn from(item: WorkshopItem) -> Self {
		Addon::Workshop(item)
	}
}
pub struct ErrorReporter;
impl<R: tauri::Runtime> tauri::plugin::Plugin<R> for ErrorReporter {
	fn initialization_script(&self) -> Option<String> {
		Some(include_str!("../../app/plugins/ErrorReporter.js").replacen(
			"{$_DEBUG_MODE_$}",
			if cfg!(debug_assertions) { "true" } else { "false" },
			1,
		))
	}

	fn name(&self) -> &'static str {
		"ErrorReporter"
	}
}

#[tauri::command]
pub fn js_error(message: String, stack: String) {
	eprintln!("[WebView] JavaScript error: {}\n{}", message, stack);
}

#[tauri::command]
pub fn error(message: String) {
	eprintln!("[WebView] {}", message);
}

#[tauri::command]
pub fn info(message: String) {
	println!("[WebView] {}", message);
}

#[tauri::command]
pub fn warn(message: String) {
	wprintln!("[WebView] {}", message);
}

static RELOADED: AtomicBool = AtomicBool::new(false);
#[tauri::command]
pub fn reloaded() -> &'static crate::appdata::AppData {
	if RELOADED.fetch_or(true, std::sync::atomic::Ordering::SeqCst) {
		crate::commands::free_caches();
	}
	&crate::APP_DATA
}
