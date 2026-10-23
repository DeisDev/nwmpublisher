use std::{
	collections::BinaryHeap,
	path::{Path, PathBuf},
	sync::Arc,
};

use rayon::ThreadPool;
use serde::Serialize;

use crate::{
	gma::{AddonType, GMAMetadata},
	transactions::Transaction,
	webview::Addon,
};

static THREAD_POOL: std::sync::LazyLock<ThreadPool> = std::sync::LazyLock::new(|| thread_pool!(4));

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AddonFolder {
	path: PathBuf,
	title: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AnalyzedItem {
	size: u64,
	#[serde(skip)]
	tag: AddonType,
	#[serde(skip_serializing_if = "Option::is_none")]
	archive: Option<Arc<Addon>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	folder: Option<AddonFolder>,
}
impl AnalyzedItem {
	fn archive(addon: Arc<Addon>) -> Self {
		let gma = addon.installed();
		Self { size: gma.size, tag: AddonType::of(gma.metadata.as_ref()), archive: Some(addon), folder: None }
	}
}
impl PartialEq for AnalyzedItem {
	fn eq(&self, other: &Self) -> bool {
		self.size == other.size
	}
}
impl Eq for AnalyzedItem {}
impl PartialOrd for AnalyzedItem {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		Some(self.cmp(other))
	}
}
impl Ord for AnalyzedItem {
	fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		self.size.cmp(&other.size).reverse()
	}
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalyzedTag {
	tag: AddonType,
	size: u64,
	items: Vec<AnalyzedItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AddonSizeAnalysis {
	total_size: u64,
	tags: Vec<AnalyzedTag>,
}

struct ProgressRange {
	start: f64,
	span: f64,
}
impl ProgressRange {
	fn report(&self, transaction: &Transaction, i: usize, total: usize) {
		if i.is_multiple_of((total / 100).max(1)) {
			transaction.progress(self.start + self.span * (i as f64 / total.max(1) as f64));
		}
	}
}

fn folder_size(folder: &Path) -> u64 {
	walkdir::WalkDir::new(folder)
		.into_iter()
		.filter_map(Result::ok)
		.filter(|entry| entry.file_type().is_file())
		.filter_map(|entry| entry.metadata().ok())
		.map(|metadata| metadata.len())
		.sum()
}

/// Unpacked addons are the folders directly inside the game's addons folder.
fn measure_addon_folders(addons_dir: &Path, transaction: &Transaction, progress: &ProgressRange) -> Vec<AnalyzedItem> {
	let Ok(entries) = addons_dir.read_dir() else { return Vec::new() };
	let folders: Vec<PathBuf> = entries
		.filter_map(Result::ok)
		.filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
		.map(|entry| entry.path())
		.collect();

	let total = folders.len();
	let mut items = Vec::with_capacity(total);
	for (i, folder) in folders.into_iter().enumerate() {
		if transaction.aborted() {
			break;
		}
		progress.report(transaction, i, total);

		let metadata = std::fs::read(folder.join("addon.json")).ok().and_then(|json| serde_json::from_slice::<GMAMetadata>(&json).ok());
		let folder_name = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
		let title = metadata.as_ref().map(|metadata| metadata.title().trim()).filter(|title| !title.is_empty()).map_or(folder_name, str::to_owned);

		items.push(AnalyzedItem {
			size: folder_size(&folder),
			tag: AddonType::of(metadata.as_ref()),
			archive: None,
			folder: Some(AddonFolder { path: dunce::canonicalize(&folder).unwrap_or(folder), title }),
		});
	}
	items
}

fn sort_by_size(items: Vec<AnalyzedItem>, transaction: &Transaction, progress: &ProgressRange) -> (Vec<AnalyzedItem>, u64) {
	let mut sorted = BinaryHeap::with_capacity(items.len());
	let mut total_size = 0;

	let total = items.len();
	for (i, item) in items.into_iter().enumerate() {
		total_size += item.size;
		sorted.push(item);
		progress.report(transaction, i, total);
	}

	(sorted.into_sorted_vec(), total_size)
}

fn group_by_tag(items: Vec<AnalyzedItem>, transaction: &Transaction, progress: &ProgressRange) -> Vec<AnalyzedTag> {
	let mut tags: Vec<AnalyzedTag> = Vec::new();

	let total = items.len();
	for (i, item) in items.into_iter().enumerate() {
		match tags.iter_mut().find(|group| group.tag == item.tag) {
			Some(group) => {
				group.size += item.size;
				group.items.push(item);
			}
			None => tags.push(AnalyzedTag { tag: item.tag, size: item.size, items: vec![item] }),
		}
		progress.report(transaction, i, total);
	}

	// Squarified treemaps need the largest groups first.
	tags.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.tag.cmp(&b.tag)));
	tags
}

pub struct AddonSizeAnalyzer;
impl AddonSizeAnalyzer {
	pub fn init() -> Self {
		Self {}
	}

	pub fn compute(&'static self, include_folders: bool, rescan: bool) -> Transaction {
		let transaction = transaction!();
		let transaction_ref = transaction.clone();

		THREAD_POOL.spawn(move || {
			transaction.status("FS_ANALYZER_DISCOVERING");
			if rescan {
				crate::game_addons::free_caches();
			}

			let addons = game_addons!().visible_addons();
			let mut items: Vec<AnalyzedItem> = addons.iter().cloned().map(AnalyzedItem::archive).collect();
			steam!().fetch_workshop_items(addons.iter().filter_map(|addon| addon.installed().id).collect());

			let mut next = 0.;
			if include_folders {
				transaction.status("FS_ANALYZER_FOLDERS");
				if let Some(gmod) = app_data!().gmod_dir() {
					items.extend(measure_addon_folders(&gmod.join("GarrysMod/addons"), &transaction, &ProgressRange { start: 0., span: 0.4 }));
				}
				next = 0.4;
			}
			if transaction.aborted() {
				return;
			}

			if items.is_empty() {
				transaction.error("ERR_NO_ADDONS_FOUND", turbonone!());
				return;
			}

			let span = (1. - next) / 2.;
			transaction.status("FS_ANALYZER_COMPUTING");
			let (items, total_size) = sort_by_size(items, &transaction, &ProgressRange { start: next, span });
			if transaction.aborted() {
				return;
			}

			transaction.status("FS_ANALYZER_TAGGIFYING");
			let tags = group_by_tag(items, &transaction, &ProgressRange { start: next + span, span });
			if transaction.aborted() {
				return;
			}

			transaction.status("FS_ANALYZER_SERIALIZING");
			transaction.progress(1.);
			transaction.finished(Some(AddonSizeAnalysis { total_size, tags }));
		});

		transaction_ref
	}
}

#[tauri::command]
pub fn addon_size_analyzer(include_folders: bool, rescan: bool) -> Transaction {
	crate::ADDON_SIZE_ANALYZER.compute(include_folders, rescan)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::gma::{GMAFile, GMAFilePointers};
	use steamworks::PublishedFileId;

	const PROGRESS: ProgressRange = ProgressRange { start: 0., span: 1. };

	fn gma(size: u64, id: u64, metadata: Option<GMAMetadata>) -> Arc<Addon> {
		Arc::new(Addon::Installed(GMAFile {
			path: format!("{id}.gma").into(),
			size,
			id: Some(PublishedFileId(id)),
			metadata,
			entries: None,
			pointers: GMAFilePointers::default(),
			version: 3,
			extracted_name: String::new(),
			modified: None,
			membuffer: None,
			spool: None,
		}))
	}

	fn typed(size: u64, id: u64, addon_type: &str) -> AnalyzedItem {
		AnalyzedItem::archive(gma(size, id, Some(GMAMetadata::Standard { title: String::new(), addon_type: addon_type.into(), tags: Vec::new(), ignore: Vec::new() })))
	}

	#[test]
	fn analyzer_orders_items_by_size_instead_of_id() {
		let items = BinaryHeap::from(vec![typed(25, 999, "tool"), typed(900, 1, "tool"), typed(100, 500, "tool")]).into_sorted_vec();
		let sizes: Vec<_> = items.iter().map(|item| item.size).collect();
		assert_eq!(sizes, [900, 100, 25]);
		assert_eq!(items[0].partial_cmp(&items[1]), Some(items[0].cmp(&items[1])));
	}

	#[test]
	fn equally_sized_items_compare_equal() {
		let first = typed(100, 1, "tool");
		let second = typed(100, 2, "map");
		assert_eq!(first.cmp(&second), std::cmp::Ordering::Equal);
		assert_eq!(first.partial_cmp(&second), Some(std::cmp::Ordering::Equal));
		assert_eq!(first, second);
	}

	#[test]
	fn tags_are_grouped_and_sorted_by_total_size() {
		let transaction = transaction!();
		let items = vec![typed(500, 1, "map"), typed(300, 2, "Tool"), typed(250, 3, "tool"), typed(100, 4, "weapon")];
		let (sorted, total_size) = sort_by_size(items, &transaction, &PROGRESS);
		assert_eq!(total_size, 1150);

		let tags = group_by_tag(sorted, &transaction, &PROGRESS);
		let summary: Vec<_> = tags.iter().map(|tag| (tag.tag, tag.size, tag.items.len())).collect();
		assert_eq!(summary, [(AddonType::Tool, 550, 2), (AddonType::Map, 500, 1), (AddonType::Weapon, 100, 1)]);
		assert_eq!(tags[0].items[0].size, 300);
	}

	#[test]
	fn unreadable_or_unknown_metadata_falls_back_to_other() {
		let transaction = transaction!();
		let items = vec![
			AnalyzedItem::archive(gma(10, 1, None)),
			typed(20, 2, ""),
			typed(30, 3, "Server Content"),
			AnalyzedItem::archive(gma(40, 4, Some(GMAMetadata::Legacy { title: String::new(), description: String::new() }))),
		];
		let (sorted, _) = sort_by_size(items, &transaction, &PROGRESS);
		let tags = group_by_tag(sorted, &transaction, &PROGRESS);
		assert_eq!(tags.len(), 1);
		assert_eq!(tags[0].tag, AddonType::Other);
		assert_eq!(tags[0].size, 100);
	}

	#[test]
	fn addon_folders_are_measured_with_their_metadata() {
		let dir = tempfile::tempdir().unwrap();
		let typed_folder = dir.path().join("my_map");
		std::fs::create_dir_all(typed_folder.join("maps")).unwrap();
		std::fs::write(typed_folder.join("addon.json"), br#"{"title":"My Map","type":"map","tags":["fun"],"ignore":[]}"#).unwrap();
		std::fs::write(typed_folder.join("maps/a.bsp"), [0; 10]).unwrap();
		let plain_folder = dir.path().join("plain");
		std::fs::create_dir_all(&plain_folder).unwrap();
		std::fs::write(plain_folder.join("x.lua"), [0; 5]).unwrap();
		std::fs::write(dir.path().join("archive.gma"), [0; 7]).unwrap();

		let transaction = transaction!();
		let mut items = measure_addon_folders(dir.path(), &transaction, &PROGRESS);
		items.sort();

		let folder_json_size = std::fs::metadata(typed_folder.join("addon.json")).unwrap().len();
		let summary: Vec<_> = items.iter().map(|item| (item.folder.as_ref().unwrap().title.as_str(), item.tag, item.size)).collect();
		assert_eq!(summary, [("My Map", AddonType::Map, 10 + folder_json_size), ("plain", AddonType::Other, 5)]);
		assert!(items.iter().all(|item| item.archive.is_none()));
	}
}
