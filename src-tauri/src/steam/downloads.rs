use parking_lot::{Condvar, Mutex};
use rayon::ThreadPool;

use std::{
	collections::{HashMap, HashSet, VecDeque},
	path::PathBuf,
	sync::{atomic::{AtomicBool, Ordering}, Arc},
	time::{Duration, Instant},
};

use steamworks::{ItemState, PublishedFileId, QueryResults};

use crate::{
	gma::{ExtractDestination, ExtractGMAMut},
	transactions::Transaction,
	GMAFile, GMOD_APP_ID,
};

lazy_static! {
	pub static ref DOWNLOADS: Downloads = Downloads::init();
	static ref THREAD_POOL: ThreadPool = thread_pool!(2);
}

#[derive(Debug)]
pub struct DownloadInner {
	item: PublishedFileId,
	transaction: Transaction,
	sent_total: AtomicBool,
	progress: Mutex<(u64, Instant)>,
	extract_destination: ExtractDestination,
}
impl std::hash::Hash for DownloadInner {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		self.item.hash(state);
	}
}
impl Eq for DownloadInner {}
impl PartialEq for DownloadInner {
	fn eq(&self, other: &Self) -> bool {
		self.item == other.item
	}
}
impl std::ops::Deref for DownloadInner {
	type Target = PublishedFileId;
	fn deref(&self) -> &Self::Target {
		&self.item
	}
}
pub type Download = Arc<DownloadInner>;

pub struct IDList {
	inner: Vec<PublishedFileId>,
}
impl From<IDList> for Vec<PublishedFileId> {
	fn from(val: IDList) -> Self {
		val.inner
	}
}
impl From<PublishedFileId> for IDList {
	fn from(id: PublishedFileId) -> Self {
		IDList { inner: vec![id] }
	}
}
impl From<Vec<PublishedFileId>> for IDList {
	fn from(ids: Vec<PublishedFileId>) -> Self {
		IDList { inner: ids }
	}
}

struct Scheduler {
	pending: VecDeque<Download>,
	active: HashMap<PublishedFileId, Download>,
}

pub struct Downloads {
	queue: Mutex<Scheduler>,
	watchdog: Condvar,
}
impl Downloads {
	fn init() -> Self {
		Self { queue: Mutex::new(Scheduler { pending: VecDeque::new(), active: HashMap::new() }), watchdog: Condvar::new() }
	}

	fn admit_ready(&self, slots: usize) -> Vec<Download> {
		let mut queue = self.queue.lock();
		while queue.pending.is_empty() && queue.active.is_empty() { self.watchdog.wait(&mut queue); }
		let mut admitted = Vec::new();
		while queue.active.len() < slots {
			let Some(job) = queue.pending.pop_front() else { break; };
			if job.transaction.aborted() { continue; }
			*job.progress.lock() = (0, Instant::now());
			queue.active.insert(job.item, job.clone());
			admitted.push(job);
		}
		admitted
	}

	fn failure(item: PublishedFileId, message: &str, detail: String) {
		let transaction = transaction!();
		transaction.context(serde_json::json!({ "kind": "download", "workshopId": item }));
		webview_emit!("DownloadStarted", transaction.id);
		transaction.data((0, item));
		transaction.error(message, detail);
	}

	fn extract(folder: PathBuf, item: PublishedFileId, extract_destination: ExtractDestination) {
		let transaction = crate::transactions::new_extraction();
		transaction.context(serde_json::json!({ "kind": "extract", "workshopId": item, "sourcePath": folder }));
		transaction.status("queued");
		webview_emit!("ExtractionStarted", (transaction.id, turbonone!(), turbonone!(), Some(item)));
		THREAD_POOL.spawn(move || {
			if transaction.aborted() { return transaction.cancelled(); }
			transaction.status("locating");

			let mut gma = if folder.is_dir() {
				let mut gma_path = None;

				let read_dir = match folder.read_dir() {
					Ok(entries) => entries,
					Err(error) => return transaction.error(crate::GMAError::io("read download directory", &folder, error).to_string(), turbonone!()),
				};
				for entry in read_dir {
					let entry = match entry {
						Ok(entry) => entry,
						Err(error) => return transaction.error(crate::GMAError::io("read download entry", &folder, error).to_string(), turbonone!()),
					};
					if !crate::path::has_extension(entry.path(), "gma") {
						continue;
					}
					if gma_path.is_some() {
						gma_path = None;
						break;
					}
					gma_path = Some(entry.path());
				}

				if let Some(path) = gma_path {
					match GMAFile::open(path) {
						Ok(gma) => gma,
						Err(err) => return transaction.error(err.to_string(), turbonone!()),
					}
				} else {
					return transaction.error("ERR_DOWNLOAD_MISSING", turbonone!());
				}
			} else if folder.is_file() && crate::path::has_extension(&folder, "bin") {
				match GMAFile::open(&folder) {
					Ok(gma) => gma,
					Err(crate::GMAError::InvalidHeader) => {
						transaction.status("decompressing");
						match GMAFile::decompress(folder, transaction.clone()) {
							Ok(gma) => {
								transaction.progress_reset();
								gma
							}
							Err(crate::GMAError::Cancelled) => return transaction.cancelled(),
							Err(err) => return transaction.error(err.to_string(), turbonone!()),
						}
					}
					Err(error) => return transaction.error(error.to_string(), turbonone!()),
				}
			} else {
				return transaction.error("ERR_DOWNLOAD_MISSING", turbonone!());
			};

			gma.id = Some(item);

			transaction.status("reading_metadata");
			transaction.data((Some(gma.metadata.as_ref().map(|metadata| metadata.title().to_owned())), gma.size));

			let _ = gma.extract(extract_destination, &transaction, false, true);
		});
	}

	fn push_download(item: PublishedFileId, destination: &ExtractDestination) {
		let mut queue = downloads!().queue.lock();
		if queue.active.contains_key(&item) || queue.pending.iter().any(|job| job.item == item) {
			drop(queue);
			Self::failure(item, "ERR_DOWNLOAD_DUPLICATE", item.0.to_string());
			return;
		}
		let transaction = transaction!();
		transaction.context(serde_json::json!({ "kind": "download", "workshopId": item }));
		webview_emit!("DownloadStarted", transaction.id);
		transaction.data((0, item));
		queue.pending.push_back(Arc::new(DownloadInner {
			item, transaction, sent_total: AtomicBool::new(false), progress: Mutex::new((0, Instant::now())), extract_destination: destination.clone(),
		}));
		downloads!().watchdog.notify_one();
	}

	pub fn download<IDs: Into<IDList>>(&self, ids: IDs) {
		let ids: Vec<PublishedFileId> = ids.into().into();
		let destination = app_data!().settings.read().extract_destination.clone();
		std::thread::spawn(move || {
			let mut seen = HashSet::new();
			let mut pending: VecDeque<_> = ids.into_iter().filter(|id| seen.insert(*id)).collect();
			while !pending.is_empty() {
				let batch: Vec<_> = pending.drain(..pending.len().min(50)).collect();
				if !steam!().connected() {
					for id in batch { Self::failure(id, "ERR_STEAM_ERROR", "Steam is disconnected".into()); }
					continue;
				}
				let query = match steam!().client().ugc().query_items(batch.clone()) {
					Ok(query) => query,
					Err(error) => { for id in batch { Self::failure(id, "ERR_STEAM_ERROR", error.to_string()); } continue; }
				};
				let reply = Arc::new(Mutex::new(None));
				let callback = reply.clone();
				let expected = batch.clone();
				query.include_children(true).fetch(move |result: Result<QueryResults<'_>, steamworks::SteamError>| {
					let result = result.map_err(|error| error.to_string()).map(|results| {
						expected.iter().enumerate().map(|(index, id)| {
							let item = results.get(index as u32).ok_or_else(|| "ERR_ITEM_NOT_FOUND".to_owned())?;
							if item.file_type == steamworks::FileType::Collection {
								Ok((*id, Some(results.get_children(index as u32).ok_or_else(|| "ERR_COLLECTION_EXPANSION".to_owned())?)))
							} else { Ok((*id, None)) }
						}).collect::<Vec<Result<(PublishedFileId, Option<Vec<PublishedFileId>>), String>>>()
					});
					*callback.lock() = Some(result);
				});
				let started = Instant::now();
				let result = loop {
					if let Some(reply) = reply.lock().take() { break reply; }
					if started.elapsed() >= Duration::from_secs(120) { break Err("ERR_COLLECTION_STALLED".into()); }
					steam!().run_callbacks();
				};
				match result {
					Err(error) => for id in batch { Self::failure(id, "ERR_COLLECTION_EXPANSION", error.clone()); },
					Ok(items) => for (index, item) in items.into_iter().enumerate() {
						match item {
							Ok((_, Some(children))) => for child in children { if seen.insert(child) { pending.push_back(child); } },
							Ok((id, None)) => Self::push_download(id, &destination),
							Err(error) => Self::failure(batch[index], "ERR_COLLECTION_EXPANSION", error),
						}
					},
				}
			}
		});
	}

	fn complete(download: Download, error: Option<steamworks::SteamError>) {
		if download.transaction.aborted() { return; }
		if let Some(error) = error { download.transaction.error("ERR_STEAM_ERROR", error); }
		else if let Some(info) = steam!().client().ugc().item_install_info(download.item) {
			download.transaction.finished(turbonone!());
			Self::extract(PathBuf::from(info.folder), download.item, download.extract_destination.clone());
		} else { download.transaction.error("ERR_DOWNLOAD_MISSING", turbonone!()); }
	}

	pub(super) fn watchdog() {
		const DOWNLOAD_SLOTS: usize = 4;
		let _callback = steam!().register_callback(move |result: steamworks::DownloadItemResult| {
			if result.app_id != GMOD_APP_ID { return; }
			let download = downloads!().queue.lock().active.remove(&result.published_file_id);
			if let Some(download) = download { Self::complete(download, result.error); }
			downloads!().watchdog.notify_one();
		});
		loop {
			let admitted = DOWNLOADS.admit_ready(DOWNLOAD_SLOTS);
			let ugc = steam!().client().ugc();
			for job in admitted {
				let state = ugc.item_state(job.item);
				if state.contains(ItemState::INSTALLED) && !state.intersects(ItemState::NEEDS_UPDATE) {
					DOWNLOADS.queue.lock().active.remove(&job.item);
					Self::complete(job, None);
				} else if !ugc.download_item(job.item, true) {
					DOWNLOADS.queue.lock().active.remove(&job.item);
					job.transaction.error("ERR_DOWNLOAD_FAILED", turbonone!());
				}
			}
			let active: Vec<_> = DOWNLOADS.queue.lock().active.values().cloned().collect();
			for job in active {
				if job.transaction.aborted() { DOWNLOADS.queue.lock().active.remove(&job.item); continue; }
				if let Some((current, total)) = ugc.item_download_info(job.item) {
					let mut progress = job.progress.lock();
					if current != progress.0 { *progress = (current, Instant::now()); }
					if total > 0 {
						if !job.sent_total.swap(true, Ordering::Relaxed) { job.transaction.data((1, total)); }
						job.transaction.progress(current as f64 / total as f64);
					}
				}
				let state = ugc.item_state(job.item);
				if state.contains(ItemState::INSTALLED) && !state.intersects(ItemState::NEEDS_UPDATE | ItemState::DOWNLOADING | ItemState::DOWNLOAD_PENDING) {
					if DOWNLOADS.queue.lock().active.remove(&job.item).is_some() { Self::complete(job, None); }
				} else if job.progress.lock().1.elapsed() >= Duration::from_secs(120) {
					if DOWNLOADS.queue.lock().active.remove(&job.item).is_some() { job.transaction.error("ERR_DOWNLOAD_STALLED", job.item); }
				}
			}
			steam!().run_callbacks();
		}
	}

}

#[tauri::command]
pub fn workshop_download(ids: Vec<PublishedFileId>) {
	downloads!().download(ids);
}

#[cfg(test)]
mod tests {
	use super::*;
	fn job(id: u64) -> Download {
		Arc::new(DownloadInner {
			item: PublishedFileId(id), transaction: crate::transactions::new(), sent_total: AtomicBool::new(false),
			progress: Mutex::new((0, Instant::now())), extract_destination: ExtractDestination::Temp,
		})
	}

	#[test]
	fn available_slots_admit_work_without_waiting_for_the_previous_batch() {
		let downloads = Downloads::init();
		for id in 1..=6 { downloads.queue.lock().pending.push_back(job(id)); }
		assert_eq!(downloads.admit_ready(4).len(), 4);
		assert!(downloads.admit_ready(4).is_empty());
		downloads.queue.lock().active.remove(&PublishedFileId(2)).unwrap().transaction.finished(());
		let next = downloads.admit_ready(4);
		assert_eq!(next.len(), 1);
		assert_eq!(next[0].item, PublishedFileId(5));
		let queue = downloads.queue.lock();
		for job in queue.active.values().chain(queue.pending.iter()) { job.transaction.finished(()); }
	}

	#[test]
	fn enqueued_work_survives_notification_before_the_consumer_waits() {
		let downloads = Arc::new(Downloads::init());
		let queued = job(7);
		downloads.queue.lock().pending.push_back(queued.clone());
		downloads.watchdog.notify_one();
		let (sender, receiver) = std::sync::mpsc::channel();
		let worker = downloads.clone();
		let handle = std::thread::spawn(move || { sender.send(worker.admit_ready(4).len()).unwrap(); });
		assert_eq!(receiver.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
		handle.join().unwrap();
		queued.transaction.finished(());
	}
}
