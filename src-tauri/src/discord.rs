use std::{
	collections::HashMap,
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc,
	},
	time::Duration,
};

use discord_presence::{event_handler::EventCallbackHandle, Client};
use parking_lot::Mutex;
use serde::Deserialize;

use crate::transactions::JobState;

const APPLICATION_ID: Option<u64> = Some(1557645809238151178);
static STOPPED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
	MyWorkshop,
	InstalledAddons,
	Downloader,
	SizeAnalyzer,
	AddonCleaner,
	PreparingAddon,
	EditingAddon,
	Settings,
	Publishing,
	Packaging,
	Downloading,
	Extracting,
	Cancelling,
}

const ACTIVITIES: [Activity; 13] = [
	Activity::MyWorkshop,
	Activity::InstalledAddons,
	Activity::Downloader,
	Activity::SizeAnalyzer,
	Activity::AddonCleaner,
	Activity::PreparingAddon,
	Activity::EditingAddon,
	Activity::Settings,
	Activity::Publishing,
	Activity::Packaging,
	Activity::Downloading,
	Activity::Extracting,
	Activity::Cancelling,
];

struct Presence {
	screen: Activity,
	labels: HashMap<Activity, String>,
}

lazy_static! {
	static ref PRESENCE: Mutex<Presence> = Mutex::new(Presence {
		screen: Activity::MyWorkshop,
		labels: HashMap::new()
	});
}

#[tauri::command]
pub fn discord_configured() -> bool {
	APPLICATION_ID.is_some_and(|id| id > 0)
}

#[tauri::command]
pub fn update_discord_presence(screen: Activity, labels: HashMap<Activity, String>) -> Result<(), String> {
	if !ACTIVITIES.iter().all(|activity| {
		labels
			.get(activity)
			.is_some_and(|label| label.chars().count() >= 2 && !label.chars().any(char::is_control))
	}) {
		return Err("ERR_DISCORD_ACTIVITY".into());
	}
	*PRESENCE.lock() = Presence { screen, labels };
	Ok(())
}

fn job_activity(kind: &str, state: JobState) -> Option<(u8, Activity)> {
	if matches!(state, JobState::Finished | JobState::Failed | JobState::Cancelled | JobState::Unknown) {
		return None;
	}
	let (priority, activity) = match kind {
		"publish" if state == JobState::Packing => (4, Activity::Packaging),
		"publish" => (4, Activity::Publishing),
		"package" => (3, Activity::Packaging),
		"extract" | "local_extract" => (2, Activity::Extracting),
		"download" => (1, Activity::Downloading),
		_ => return None,
	};
	Some((priority, if state == JobState::Cancelling { Activity::Cancelling } else { activity }))
}

fn activity_text(presence: &Presence, jobs: &[(String, JobState)]) -> Option<String> {
	let activity = jobs
		.iter()
		.filter_map(|(kind, state)| job_activity(kind, *state))
		.max_by_key(|(priority, _)| *priority)
		.map_or(presence.screen, |(_, activity)| activity);
	presence.labels.get(&activity).map(|label| {
		let mut end = label.len().min(128);
		while !label.is_char_boundary(end) {
			end -= 1;
		}
		label[..end].to_owned()
	})
}

struct Connection {
	client: Client,
	resend: Arc<AtomicBool>,
	_connected: EventCallbackHandle,
	_error: EventCallbackHandle,
	last_text: Option<String>,
}

impl Connection {
	fn start(id: u64) -> Self {
		let mut client = Client::new(id);
		let resend = Arc::new(AtomicBool::new(true));
		let connected_resend = resend.clone();
		let last_error = Arc::new(Mutex::new(String::new()));
		let connected_error = last_error.clone();
		let connected = client.on_connected(move |_| {
			connected_resend.store(true, Ordering::Release);
			connected_error.lock().clear();
			println!("[Discord] Connected");
		});
		let error = client.on_error(move |context| {
			let message = format!("{:?}", context.event);
			let mut previous = last_error.lock();
			if *previous != message {
				eprintln!("[Discord] {message}");
				*previous = message;
			}
		});
		client.start();
		Self {
			client,
			resend,
			_connected: connected,
			_error: error,
			last_text: None,
		}
	}

	fn update(&mut self, text: String) {
		if self.resend.swap(false, Ordering::AcqRel) || self.last_text.as_ref() != Some(&text) {
			self.client.queue_activity(|activity| activity.details(text.clone()));
			self.last_text = Some(text);
		}
	}

	fn stop(self) {
		if let Err(error) = self.client.shutdown() {
			eprintln!("[Discord] Shutdown failed: {error}");
		}
	}
}

pub fn start() {
	let Some(id) = APPLICATION_ID.filter(|id| *id > 0) else {
		println!("[Discord] Application ID is not configured");
		return;
	};
	std::thread::spawn(move || {
		let mut connection: Option<Connection> = None;
		while !STOPPED.load(Ordering::Acquire) {
			let enabled = app_data!().settings.read().discord_rich_presence;
			if enabled {
				let jobs = crate::transactions::snapshots::active_job_kinds();
				if let Some(text) = activity_text(&PRESENCE.lock(), &jobs) {
					connection.get_or_insert_with(|| Connection::start(id)).update(text);
				}
			} else if let Some(connection) = connection.take() {
				connection.stop();
			}
			std::thread::sleep(Duration::from_millis(250));
		}
		if let Some(connection) = connection {
			connection.stop();
		}
	});
}

pub fn stop() {
	STOPPED.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
	use super::*;

	fn presence() -> Presence {
		Presence {
			screen: Activity::EditingAddon,
			labels: ACTIVITIES.into_iter().map(|activity| (activity, format!("{activity:?}"))).collect(),
		}
	}

	#[test]
	fn jobs_override_screens_and_finished_jobs_restore_them() {
		let presence = presence();
		assert_eq!(activity_text(&presence, &[]).as_deref(), Some("EditingAddon"));
		assert_eq!(
			activity_text(
				&presence,
				&[("download".into(), JobState::Running), ("publish".into(), JobState::Packing)]
			)
			.as_deref(),
			Some("Packaging")
		);
		assert_eq!(
			activity_text(&presence, &[("publish".into(), JobState::Submitting)]).as_deref(),
			Some("Publishing")
		);
		assert_eq!(
			activity_text(&presence, &[("package".into(), JobState::Committing)]).as_deref(),
			Some("Packaging")
		);
		for kind in ["extract", "local_extract"] {
			assert_eq!(
				activity_text(&presence, &[(kind.into(), JobState::Running)]).as_deref(),
				Some("Extracting")
			);
		}
		assert_eq!(
			activity_text(&presence, &[("download".into(), JobState::Cancelling)]).as_deref(),
			Some("Cancelling")
		);
		for state in [JobState::Finished, JobState::Failed, JobState::Cancelled, JobState::Unknown] {
			assert_eq!(activity_text(&presence, &[("publish".into(), state)]).as_deref(), Some("EditingAddon"));
		}
		assert_eq!(
			activity_text(&presence, &[("unrelated".into(), JobState::Running)]).as_deref(),
			Some("EditingAddon")
		);
	}

	#[test]
	fn presence_text_is_localized_bounded_and_contains_no_job_details() {
		let mut presence = presence();
		presence.labels.insert(Activity::Downloading, "下载🦀".repeat(50));
		let text = activity_text(&presence, &[("download".into(), JobState::Running)]).unwrap();
		assert!(text.len() <= 128);
		assert!("下载🦀".repeat(50).starts_with(&text));
		presence.labels.clear();
		assert!(activity_text(&presence, &[]).is_none());
	}

	#[test]
	fn presence_requires_complete_valid_labels() {
		assert!(update_discord_presence(Activity::MyWorkshop, HashMap::new()).is_err());
		let mut labels = presence().labels;
		labels.insert(Activity::Publishing, String::new());
		assert!(update_discord_presence(Activity::MyWorkshop, labels).is_err());
	}
}
