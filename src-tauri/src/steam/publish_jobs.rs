use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::Write, path::{Path, PathBuf}, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};
use steamworks::PublishedFileId;
use crate::Transaction;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
	pub key: String,
	pub outcome: String,
	pub workshop_id: Option<PublishedFileId>,
	pub source: Option<PathBuf>,
	pub staging: Option<PathBuf>,
	pub cleanup_warnings: Vec<String>,
	pub recoverability: String,
	pub started: u64,
	pub remote_started: bool,
	pub reviewed: bool,
}

#[derive(Clone)]
pub struct PublishJob(Arc<Mutex<PublishResult>>);

lazy_static! {
	static ref JOBS_LOCK: Mutex<()> = Mutex::new(());
	static ref CURRENT_JOBS: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
}

fn directory() -> PathBuf { app_data!().user_data_dir().join("publishing-jobs") }

fn save(record: &PublishResult) -> Result<(), String> {
	let directory = directory();
	fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
	let mut file = tempfile::NamedTempFile::new_in(&directory).map_err(|error| error.to_string())?;
	serde_json::to_writer(&mut file, record).map_err(|error| error.to_string())?;
	file.flush().and_then(|_| file.as_file().sync_all()).map_err(|error| error.to_string())?;
	file.persist(directory.join(format!("{}.json", record.key))).map_err(|error| error.error.to_string())?;
	Ok(())
}

fn records() -> Result<Vec<PublishResult>, String> {
	let directory = directory();
	let entries = match fs::read_dir(&directory) {
		Ok(entries) => entries, Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()), Err(error) => return Err(error.to_string()),
	};
	let mut records = Vec::new();
	for entry in entries {
		let path = entry.map_err(|error| error.to_string())?.path();
		if path.extension().is_none_or(|extension| extension != "json") { continue; }
		let file = fs::File::open(&path).map_err(|error| error.to_string())?;
		records.push(serde_json::from_reader(file).map_err(|error| format!("{}: {}", path.display(), error))?);
	}
	Ok(records)
}

impl PublishJob {
	pub fn new(transaction: &Transaction, id: Option<PublishedFileId>, source: Option<PathBuf>) -> Result<Self, String> {
		let source = source.map(dunce::canonicalize).transpose().map_err(|error| error.to_string())?;
		let _guard = JOBS_LOCK.lock();
		for previous in records()? {
			let same_source = source.is_some() && previous.source == source;
			let same_item = id.is_some() && previous.workshop_id == id;
			if (same_source || same_item) && matches!(previous.outcome.as_str(), "pending" | "unknown") && !previous.reviewed {
				return Err(format!("ERR_PUBLISH_UNRESOLVED:{}", previous.workshop_id.map_or(previous.key, |id| id.0.to_string())));
			}
			if id.is_none() && same_source && previous.workshop_id.is_some() {
				return Err(format!("ERR_PUBLISH_EXISTING_ITEM:{}", previous.workshop_id.unwrap().0));
			}
		}
		let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?;
		let record = PublishResult {
			key: format!("{}-{}-{}", now.as_nanos(), std::process::id(), transaction.id), outcome: "pending".into(), workshop_id: id,
			source, staging: None, cleanup_warnings: Vec::new(), recoverability: "wait".into(), started: now.as_secs(), remote_started: false, reviewed: false,
		};
		save(&record)?;
		CURRENT_JOBS.lock().insert(record.key.clone());
		transaction.context(serde_json::json!({ "kind": "publish", "operationKey": record.key, "workshopId": id }));
		Ok(Self(Arc::new(Mutex::new(record))))
	}

	fn update(&self, transaction: &Transaction, update: impl FnOnce(&mut PublishResult)) {
		let _guard = JOBS_LOCK.lock();
		let mut record = self.0.lock();
		update(&mut record);
		if let Err(error) = save(&record) { transaction.warning(format!("ERR_PUBLISH_JOURNAL:{}", error)); }
	}

	pub fn staging(&self, root: &Path) -> Result<(), String> {
		let _guard = JOBS_LOCK.lock();
		let mut record = self.0.lock();
		record.staging = Some(root.to_owned());
		fs::write(root.join("operation-id"), &record.key).map_err(|error| error.to_string())?;
		save(&record)
	}

	pub fn submitting(&self) -> Result<(), String> {
		let _guard = JOBS_LOCK.lock();
		let mut record = self.0.lock();
		record.remote_started = true;
		save(&record)
	}

	pub fn created(&self, id: PublishedFileId, transaction: &Transaction) {
		self.update(transaction, |record| { record.workshop_id = Some(id); });
		let record = self.0.lock().clone();
		transaction.context(serde_json::json!({ "kind": "publish", "operationKey": record.key, "workshopId": id }));
		if let Some(path) = record.source {
			if let Err(error) = crate::appdata::change_settings(|settings| { settings.my_workshop_local_paths.insert(id, path); Ok(()) }) {
				transaction.warning(error);
			}
			app_data!().send();
		}
	}

	pub fn uncertain(&self, transaction: &Transaction) {
		if self.0.lock().outcome == "unknown" { return; }
		self.update(transaction, |record| { record.outcome = "unknown".into(); record.recoverability = "inspect_workshop".into(); });
		transaction.uncertain();
	}

	pub fn finish(&self, transaction: &Transaction, outcome: &str, warnings: Vec<String>) -> PublishResult {
		self.update(transaction, |record| {
			record.outcome = outcome.into();
			record.cleanup_warnings.extend(warnings);
			record.recoverability = if outcome == "published" { "cleanup" } else if record.workshop_id.is_some() { "retry_update" } else { "retry" }.into();
		});
		let record = self.0.lock().clone();
		CURRENT_JOBS.lock().remove(&record.key);
		record
	}
}

#[tauri::command]
pub fn publish_operations() -> Result<Vec<PublishResult>, String> {
	let _guard = JOBS_LOCK.lock();
	let current = CURRENT_JOBS.lock();
	Ok(records()?.into_iter().filter(|record| record.outcome == "unknown" || (record.outcome == "pending" && !current.contains(&record.key)) || !record.cleanup_warnings.is_empty()).collect())
}

#[tauri::command]
pub async fn reconcile_publish(key: String) -> Result<serde_json::Value, String> {
	tauri::async_runtime::spawn_blocking(move || {
		let record = records()?.into_iter().find(|record| record.key == key).ok_or("ERR_TRANSACTION_NOT_FOUND")?;
		let id = record.workshop_id.ok_or("ERR_PUBLISH_ID_UNKNOWN")?;
		let (sender, receiver) = std::sync::mpsc::sync_channel(1);
		steam!().client().ugc().query_item(id).map_err(|error| error.to_string())?.allow_cached_response(0).include_long_desc(true)
			.fetch(move |result| {
				let evidence = result.map_err(|error| error.to_string()).and_then(|results| {
					let item = results.get(0).ok_or("ERR_ITEM_NOT_FOUND")?;
					Ok(serde_json::json!({ "workshopId": id, "title": item.title, "description": item.description, "updated": item.time_updated, "fileSize": item.file_size }))
				});
				let _ = sender.send(evidence);
			});
		receiver.recv_timeout(Duration::from_secs(10)).map_err(|_| "ERR_PUBLISH_RECONCILE_TIMEOUT".to_owned())?
	}).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn review_publish(key: String, workshop_id: String) -> Result<(), String> {
	let id = PublishedFileId(workshop_id.parse::<u64>().map_err(|_| "ERR_ITEM_NOT_FOUND")?);
	if id.0 == 0 { return Err("ERR_ITEM_NOT_FOUND".into()); }
	tauri::async_runtime::spawn_blocking(move || {
		let (sender, receiver) = std::sync::mpsc::sync_channel(1);
		steam!().client().ugc().query_item(id).map_err(|error| error.to_string())?.allow_cached_response(0).fetch(move |result| {
			let result = result.map_err(|error| error.to_string()).and_then(|results| {
				let item = results.get(0).ok_or("ERR_ITEM_NOT_FOUND")?;
				if item.consumer_app_id != Some(crate::GMOD_APP_ID) || item.owner != steam!().client().steam_id {
					return Err("ERR_PUBLISH_RECOVERY_OWNER".into());
				}
				Ok(())
			});
			let _ = sender.send(result);
		});
		receiver.recv_timeout(Duration::from_secs(10)).map_err(|_| "ERR_PUBLISH_RECONCILE_TIMEOUT")??;
		let _guard = JOBS_LOCK.lock();
		let mut record = records()?.into_iter().find(|record| record.key == key).ok_or("ERR_TRANSACTION_NOT_FOUND")?;
		if record.workshop_id.is_some_and(|previous| previous != id) { return Err("ERR_PUBLISH_RECOVERY_ID".into()); }
		record.workshop_id = Some(id);
		record.reviewed = true;
		record.recoverability = "retry_update".into();
		save(&record)?;
		if let Some(path) = record.source {
			crate::appdata::change_settings(|settings| { settings.my_workshop_local_paths.insert(id, path); Ok(()) })?;
			app_data!().send();
		}
		Ok(())
	}).await.map_err(|error| error.to_string())?
}

// Cleanup only finalized, application-owned jobs. Unknown operations retain their files.
pub fn janitor() {
	std::thread::spawn(|| loop {
		std::thread::sleep(Duration::from_secs(30));
		let _guard = JOBS_LOCK.lock();
		let records = match records() { Ok(records) => records, Err(error) => { eprintln!("Publishing cleanup: {}", error); continue; } };
		for mut record in records {
			if record.outcome == "pending" && !record.remote_started && !CURRENT_JOBS.lock().contains(&record.key) {
				record.outcome = "failed".into();
				record.recoverability = "retry".into();
				if let Err(error) = save(&record) { eprintln!("Publishing cleanup: {}", error); continue; }
			}
			if !matches!(record.outcome.as_str(), "published" | "failed" | "cancelled") { continue; }
			let Some(root) = record.staging.as_ref() else { continue; };
			if !root.is_absolute() || !root.file_name().is_some_and(|name| name.to_string_lossy().starts_with("nwmpublisher-publishing-")) { continue; }
			let metadata = match fs::symlink_metadata(root) {
				Ok(metadata) => metadata,
				Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
					record.staging = None;
					record.cleanup_warnings.clear();
					if let Err(error) = save(&record) { eprintln!("Publishing cleanup: {}", error); }
					continue;
				}
				Err(error) => { eprintln!("Publishing cleanup {}: {}", root.display(), error); continue; }
			};
			if !metadata.is_dir() || metadata.file_type().is_symlink() { continue; }
			#[cfg(windows)] {
				use std::os::windows::fs::MetadataExt;
				if metadata.file_attributes() & 0x400 != 0 { continue; }
			}
			if fs::read_to_string(root.join("operation-id")).ok().as_deref() != Some(record.key.as_str()) { continue; }
			match fs::remove_dir_all(root) {
				Ok(()) => { record.staging = None; record.cleanup_warnings.clear(); if let Err(error) = save(&record) { eprintln!("Publishing cleanup: {}", error); } }
				Err(error) => eprintln!("Publishing cleanup {}: {}", root.display(), error),
			}
		}
	});
}
