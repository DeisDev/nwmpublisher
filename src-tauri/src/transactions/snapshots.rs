use super::JobState;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::{BTreeMap, VecDeque}, fs::File, io::{BufRead, BufReader, Seek, SeekFrom, Write}};

pub enum TransactionMessage {
	Finished(u32, Value), Error(u32, String, Value), Data(u32, Value), Status(u32, String),
	Progress(u32, u16), IncrProgress(u32, u16), ResetProgress(u32), State(u32, JobState), Cancelled(u32), Warning(u32, String),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
	pub id: u32,
	pub revision: u64,
	pub state: JobState,
	pub progress: u16,
	pub status: Option<String>,
	pub result: Option<Value>,
	pub error: Option<(String, Value)>,
	pub data: VecDeque<(u64, Value)>,
	pub data_more: bool,
	pub context: Option<Value>,
	pub warnings: Vec<String>,
	#[serde(skip)]
	dirty: bool,
}

struct DataSpool { file: File, read: u64, pending: usize }

lazy_static! {
	static ref SNAPSHOTS: Mutex<BTreeMap<u32, JobSnapshot>> = Mutex::new(BTreeMap::new());
	static ref DATA_SPOOLS: Mutex<BTreeMap<u32, DataSpool>> = Mutex::new(BTreeMap::new());
	// Unacknowledged outcomes outlive the frontend. Only the most recent ones stay in memory.
	static ref OUTCOMES: Result<tempfile::TempDir, std::io::Error> = tempfile::Builder::new().prefix("nwmpublisher-outcomes-").tempdir();
}

pub fn insert(id: u32, state: JobState) {
	SNAPSHOTS.lock().insert(id, JobSnapshot {
		id, revision: 0, state, progress: 0, status: None, result: None, error: None,
		data: VecDeque::new(), data_more: false, context: None, warnings: Vec::new(), dirty: true,
	});
}

pub fn context(id: u32, context: Value) {
	if let Some(snapshot) = SNAPSHOTS.lock().get_mut(&id) {
		snapshot.context = Some(context);
		snapshot.revision += 1;
		snapshot.dirty = true;
	}
}

pub fn record(message: TransactionMessage) {
	use TransactionMessage::*;
	let id = match &message {
		Finished(id, _) | Error(id, _, _) | Data(id, _) | Status(id, _) | Progress(id, _) |
		IncrProgress(id, _) | ResetProgress(id) | State(id, _) | Cancelled(id) | Warning(id, _) => *id,
	};
	let mut snapshots = SNAPSHOTS.lock();
	let Some(snapshot) = snapshots.get_mut(&id) else { return; };
	snapshot.revision += 1;
	snapshot.dirty = true;
	match message {
		Warning(_, message) => { if snapshot.warnings.len() < 32 && !snapshot.warnings.contains(&message) { snapshot.warnings.push(message); } }
		Finished(_, result) => { snapshot.state = JobState::Finished; snapshot.progress = 10000; snapshot.result = Some(result); }
		Error(_, message, data) => { snapshot.state = JobState::Failed; snapshot.error = Some((message, data)); }
		Cancelled(_) => snapshot.state = JobState::Cancelled,
		State(_, state) => snapshot.state = state,
		Status(_, status) => {
			if snapshot.status.as_ref() == Some(&status) { snapshot.revision -= 1; return; }
			snapshot.status = Some(status);
		}
		Progress(_, progress) => snapshot.progress = progress,
		IncrProgress(_, progress) => snapshot.progress = snapshot.progress.saturating_add(progress).min(10000),
		ResetProgress(_) => snapshot.progress = 0,
		Data(_, data) => {
			if data.is_number() { snapshot.data.retain(|(_, previous)| !previous.is_number()); }
			if data.as_array().is_some_and(|data| data.first().is_some_and(Value::is_null)) {
				snapshot.data.retain(|(_, previous)| !previous.as_array().is_some_and(|data| data.first().is_some_and(Value::is_null)));
			}
			let bytes = serde_json::to_vec(&snapshot.data).map_or(0, |bytes| bytes.len());
			if !snapshot.data_more && snapshot.data.len() < 128 && bytes < 1024 * 1024 {
				snapshot.data.push_back((snapshot.revision, data));
			} else {
				let result = (|| -> std::io::Result<()> {
					let mut spools = DATA_SPOOLS.lock();
					if !spools.contains_key(&id) { spools.insert(id, DataSpool { file: tempfile::tempfile()?, read: 0, pending: 0 }); }
					let spool = spools.get_mut(&id).unwrap();
					spool.file.seek(SeekFrom::End(0))?;
					serde_json::to_writer(&mut spool.file, &(snapshot.revision, data))?;
					spool.file.write_all(b"\n")?;
					spool.pending += 1;
					Ok(())
				})();
				match result {
					Ok(()) => snapshot.data_more = true,
					Err(error) => {
						let warning = format!("ERR_JOB_DATA_DELIVERY:{}", error);
						if snapshot.warnings.len() < 32 && !snapshot.warnings.contains(&warning) { snapshot.warnings.push(warning); }
					},
				}
			}
		}
	}
}

pub fn init() {
	if *crate::cli::CLI_MODE { return; }
	std::thread::spawn(|| loop {
		std::thread::sleep(std::time::Duration::from_millis(100));
		let updates: Vec<_> = {
			let mut snapshots = SNAPSHOTS.lock();
			snapshots.values_mut().filter_map(|snapshot| {
				if !snapshot.dirty { return None; }
				snapshot.dirty = false;
				Some(snapshot.clone())
			}).collect()
		};
		for snapshot in updates {
			if snapshot.state.terminal() && !snapshot.data_more {
				if let Ok(directory) = &*OUTCOMES {
					let mut snapshots = SNAPSHOTS.lock();
					if !snapshots.get(&snapshot.id).is_some_and(|current| current.revision == snapshot.revision) { continue; }
					let path = directory.path().join(format!("{}.json", snapshot.id));
					let saved = (|| -> std::io::Result<()> {
						let mut file = tempfile::NamedTempFile::new_in(directory.path())?;
						serde_json::to_writer(&mut file, &snapshot)?;
						file.as_file().sync_all()?;
						file.persist(path).map_err(|error| error.error)?;
						Ok(())
					})();
					if let Err(error) = saved { eprintln!("Save job outcome {}: {}", snapshot.id, error); }
					else { snapshots.remove(&snapshot.id); DATA_SPOOLS.lock().remove(&snapshot.id); }
				}
			}
			webview_emit!("TransactionSnapshot", snapshot);
		}
	});
}

#[tauri::command]
pub fn transaction_data(id: u32, after: u64) -> Result<Option<JobSnapshot>, String> {
	let mut snapshots = SNAPSHOTS.lock();
	let Some(snapshot) = snapshots.get_mut(&id) else { drop(snapshots); return transaction_snapshot(id); };
	snapshot.data.retain(|(sequence, _)| *sequence > after);
	let mut spools = DATA_SPOOLS.lock();
	if let Some(spool) = spools.get_mut(&id) {
		spool.file.seek(SeekFrom::Start(spool.read)).map_err(|error| error.to_string())?;
		let mut reader = BufReader::new(&mut spool.file);
		let mut bytes = 0;
		while spool.pending > 0 && snapshot.data.len() < 128 && bytes < 1024 * 1024 {
			let mut line = String::new();
			let read = reader.read_line(&mut line).map_err(|error| error.to_string())?;
			if read == 0 { return Err("ERR_JOB_DATA_DELIVERY".into()); }
			snapshot.data.push_back(serde_json::from_str(&line).map_err(|error| error.to_string())?);
			spool.read += read as u64;
			spool.pending -= 1;
			bytes += read;
		}
		drop(reader);
		snapshot.data_more = spool.pending > 0;
		if !snapshot.data_more { spools.remove(&id); }
	}
	Ok(Some(snapshot.clone()))
}

#[tauri::command]
pub fn transaction_snapshot(id: u32) -> Result<Option<JobSnapshot>, String> {
	if let Some(snapshot) = SNAPSHOTS.lock().get(&id).cloned() { return Ok(Some(snapshot)); }
	let directory = OUTCOMES.as_ref().map_err(|error| error.to_string())?;
	match std::fs::File::open(directory.path().join(format!("{id}.json"))) {
		Ok(file) => serde_json::from_reader(file).map(Some).map_err(|error| error.to_string()),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(error) => Err(error.to_string()),
	}
}

#[tauri::command]
pub fn transaction_snapshots() -> Result<Vec<JobSnapshot>, String> {
	let mut snapshots = SNAPSHOTS.lock().clone();
	let directory = OUTCOMES.as_ref().map_err(|error| error.to_string())?;
	for entry in std::fs::read_dir(directory.path()).map_err(|error| error.to_string())? {
		let entry = entry.map_err(|error| error.to_string())?;
		if let Some(id) = entry.path().file_stem().and_then(|id| id.to_str()).and_then(|id| id.parse::<u32>().ok()) {
			if let Some(snapshot) = transaction_snapshot(id)? { snapshots.insert(id, snapshot); }
		}
	}
	Ok(snapshots.into_values().collect())
}

#[tauri::command]
pub fn acknowledge_transaction(id: u32) -> Result<(), String> {
	let mut snapshots = SNAPSHOTS.lock();
	if snapshots.get(&id).is_some_and(|snapshot| !snapshot.state.terminal()) { return Ok(()); }
	if snapshots.get(&id).is_some_and(|snapshot| snapshot.state.terminal()) { snapshots.remove(&id); }
	DATA_SPOOLS.lock().remove(&id);
	if let Ok(directory) = &*OUTCOMES {
		match std::fs::remove_file(directory.path().join(format!("{id}.json"))) {
			Ok(()) => {},
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
			Err(error) => return Err(error.to_string()),
		}
	}
	Ok(())
}
