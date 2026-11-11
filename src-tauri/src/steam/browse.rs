use std::{
	collections::HashSet,
	ffi::{c_char, CStr, CString},
	mem::MaybeUninit,
	sync::atomic::{AtomicU64, Ordering},
	time::{Duration, Instant, SystemTime},
};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use steamworks::{sys, ItemState, PublishedFileId, SteamError, SteamId};

use super::{
	collections::{lookup_entries, CollectionEntry, ItemKind},
	users::SteamUser,
	workshop::{original_file_name, WorkshopItem, WorkshopVisibility},
	workshop_management::{connected, parse_reference, STEAM_TIMEOUT},
	RESULTS_PER_PAGE,
};
use crate::{webview::Addon, GMOD_APP_ID};

/// A scan stops once this many items pass the in-app filters.
const PAGE_TARGET: usize = 50;
/// Steam pages read per request at most, so strict in-app filters cannot scan forever.
const SCAN_PAGES: usize = 10;
const TEXT_MAX_BYTES: usize = 256;
const TAG_MAX_BYTES: usize = 64;
const TAGS_MAX: usize = 32;
const EXCLUDED_IDS_MAX: usize = 5000;
const USERS_MAX: usize = 100;
const SUPERSEDED: &str = "A newer search replaced this one.";
const USER_PAGE_PREFIX: &str = "page:";

static LATEST_REQUEST: AtomicU64 = AtomicU64::new(0);
static FAVORITES: Mutex<Option<(SteamId, HashSet<PublishedFileId>)>> = Mutex::new(None);

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BrowseKind {
	Addon,
	Dupe,
	Save,
	Demo,
	Collection,
}

impl BrowseKind {
	fn tag(self) -> Option<&'static str> {
		match self {
			BrowseKind::Addon => Some("addon"),
			BrowseKind::Dupe => Some("dupe"),
			BrowseKind::Save => Some("save"),
			BrowseKind::Demo => Some("demo"),
			BrowseKind::Collection => None,
		}
	}

	fn matching(self) -> sys::EUGCMatchingUGCType {
		match self {
			BrowseKind::Collection => sys::EUGCMatchingUGCType::k_EUGCMatchingUGCType_Collections,
			_ => sys::EUGCMatchingUGCType::k_EUGCMatchingUGCType_Items_ReadyToUse,
		}
	}
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BrowseSource {
	Everyone,
	Author,
	Friends,
	Followed,
	Favorites,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BrowseSort {
	Popular,
	TopRated,
	MostSubscribed,
	Newest,
	Updated,
	Relevance,
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BrowseStatus {
	#[default]
	Any,
	Subscribed,
	NotSubscribed,
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TextScope {
	#[default]
	Any,
	Title,
	Description,
}

/// Unix times; a missing end means up to now.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DateRange {
	start: Option<u32>,
	end: Option<u32>,
}

impl DateRange {
	fn bounds(&self) -> (u32, u32) {
		let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(u32::MAX, |now| now.as_secs().min(u32::MAX as u64) as u32);
		(self.start.unwrap_or(0), self.end.unwrap_or(now))
	}

	fn contains(&self, time: u32) -> bool {
		self.start.is_none_or(|start| time >= start) && self.end.is_none_or(|end| time <= end)
	}
}

fn default_trend_days() -> u32 {
	7
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowseQuery {
	kind: BrowseKind,
	source: BrowseSource,
	#[serde(default)]
	author: Option<String>,
	sort: BrowseSort,
	#[serde(default = "default_trend_days")]
	trend_days: u32,
	#[serde(default)]
	text: String,
	#[serde(default)]
	text_scope: TextScope,
	#[serde(default)]
	exact_phrase: bool,
	#[serde(default)]
	types: Vec<String>,
	#[serde(default)]
	tags: Vec<String>,
	#[serde(default)]
	match_all: bool,
	#[serde(default)]
	excluded_tags: Vec<String>,
	#[serde(default)]
	created: Option<DateRange>,
	#[serde(default)]
	updated: Option<DateRange>,
	#[serde(default)]
	size_min: Option<u64>,
	#[serde(default)]
	size_max: Option<u64>,
	#[serde(default)]
	min_subscribers: Option<u64>,
	#[serde(default)]
	min_rating: Option<f32>,
	#[serde(default)]
	status: BrowseStatus,
	#[serde(default)]
	exclude_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePage {
	total: u32,
	scanned: u32,
	items: Vec<Addon>,
	next_cursor: Option<String>,
	more: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum Preview {
	Image(String),
	Youtube(String),
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Vote {
	Up,
	Down,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseDetails {
	workshop: WorkshopItem,
	previews: Vec<Preview>,
	dependencies: Vec<CollectionEntry>,
	children: Vec<Addon>,
	total_size: Option<u64>,
	vote: Option<Vote>,
}

/// Where in Steam's results a scan continues: a cursor for queries over all items, a page for user lists.
#[derive(Debug, PartialEq)]
enum Position {
	Cursor(String),
	Page(u32),
}

impl Position {
	fn decode(user_list: bool, cursor: Option<&str>) -> Result<Self, String> {
		let invalid = || "The Workshop results changed. Search again.".to_string();
		match (user_list, cursor) {
			(false, None) => Ok(Position::Cursor("*".into())),
			(true, None) => Ok(Position::Page(1)),
			(false, Some(cursor)) => {
				if cursor.is_empty() || cursor.len() >= sys::k_cchPublishedFileURLMax as usize || cursor.contains('\0') || cursor.starts_with(USER_PAGE_PREFIX) {
					return Err(invalid());
				}
				Ok(Position::Cursor(cursor.into()))
			}
			(true, Some(cursor)) => cursor
				.strip_prefix(USER_PAGE_PREFIX)
				.and_then(|page| page.parse().ok())
				.filter(|page| *page > 0)
				.map(Position::Page)
				.ok_or_else(invalid),
		}
	}

	fn encode(&self) -> String {
		match self {
			Position::Cursor(cursor) => cursor.clone(),
			Position::Page(page) => format!("{USER_PAGE_PREFIX}{page}"),
		}
	}
}

enum Request {
	All { query: sys::EUGCQuery },
	User { account: u32, list: sys::EUserUGCList, order: sys::EUserUGCListSortOrder },
}

fn clean_list(values: &[String], what: &str) -> Result<Vec<String>, String> {
	if values.len() > TAGS_MAX {
		return Err(format!("Choose at most {TAGS_MAX} {what}."));
	}
	let mut seen = HashSet::new();
	let mut clean = Vec::with_capacity(values.len());
	for value in values {
		let value = value.trim();
		if value.is_empty() || value.len() > TAG_MAX_BYTES || value.contains(['\0', ',']) {
			return Err(format!("{value:?} is not a valid Workshop tag."));
		}
		if seen.insert(value.to_lowercase()) {
			clean.push(value.to_owned());
		}
	}
	Ok(clean)
}

/// A validated query: what Steam filters and what the app checks on each scanned item.
#[derive(Debug)]
struct Plan {
	kind: BrowseKind,
	source: BrowseSource,
	author: Option<SteamId>,
	sort: BrowseSort,
	trend_days: u32,
	text: String,
	text_scope: TextScope,
	words: Vec<String>,
	types: Vec<String>,
	tags: Vec<String>,
	match_all: bool,
	excluded_tags: Vec<String>,
	created: Option<DateRange>,
	updated: Option<DateRange>,
	size_min: Option<u64>,
	size_max: Option<u64>,
	min_subscribers: Option<u64>,
	min_rating: Option<f32>,
	status: BrowseStatus,
	exclude_ids: HashSet<PublishedFileId>,
}

impl Plan {
	fn new(query: BrowseQuery) -> Result<Self, String> {
		let text = query.text.trim().to_owned();
		if text.len() > TEXT_MAX_BYTES || text.contains('\0') {
			return Err(format!("Search text can be up to {TEXT_MAX_BYTES} bytes."));
		}
		if !(1..=365).contains(&query.trend_days) {
			return Err("The popular period must be between 1 and 365 days.".into());
		}
		let author = match (query.source, query.author.as_deref()) {
			(BrowseSource::Author, Some(author)) => Some(
				author
					.parse::<u64>()
					.ok()
					.map(SteamId::from_raw)
					.filter(|id| id.account_id().raw() != 0)
					.ok_or("Choose a valid Steam author.")?,
			),
			(BrowseSource::Author, None) => return Err("Choose an author to browse.".into()),
			(_, Some(_)) => return Err("An author can only be set when browsing one author's items.".into()),
			(_, None) => None,
		};
		let sorts: &[BrowseSort] = match query.source {
			BrowseSource::Everyone => &[
				BrowseSort::Popular,
				BrowseSort::TopRated,
				BrowseSort::MostSubscribed,
				BrowseSort::Newest,
				BrowseSort::Updated,
				BrowseSort::Relevance,
			],
			BrowseSource::Author | BrowseSource::Favorites => &[BrowseSort::Newest, BrowseSort::Updated, BrowseSort::TopRated],
			BrowseSource::Friends | BrowseSource::Followed => &[BrowseSort::Newest],
		};
		if !sorts.contains(&query.sort) {
			return Err("This sort order is not available for the chosen source.".into());
		}
		if query.sort == BrowseSort::Relevance && text.is_empty() {
			return Err("Sorting by relevance needs search text.".into());
		}
		for range in [query.created, query.updated].into_iter().flatten() {
			if let (Some(start), Some(end)) = (range.start, range.end) {
				if start > end {
					return Err("A date range must start before it ends.".into());
				}
			}
		}
		if let (Some(min), Some(max)) = (query.size_min, query.size_max) {
			if min > max {
				return Err("The minimum size must not be larger than the maximum size.".into());
			}
		}
		if query.kind == BrowseKind::Collection && (query.size_min.is_some() || query.size_max.is_some()) {
			return Err("Collections cannot be filtered by size.".into());
		}
		if query.min_rating.is_some_and(|rating| !(0.0..=1.0).contains(&rating)) {
			return Err("The minimum rating must be between 0% and 100%.".into());
		}
		if query.exclude_ids.len() > EXCLUDED_IDS_MAX {
			return Err("Too many items to skip. Search again.".into());
		}
		let exclude_ids = query
			.exclude_ids
			.iter()
			.map(|id| id.parse().ok().filter(|id| *id != 0).map(PublishedFileId).ok_or_else(|| format!("{id:?} is not a Workshop item ID.")))
			.collect::<Result<_, _>>()?;
		// Steam searches the title and description together, and only in queries over all items, so anything else is matched here.
		let steam_matches = query.source == BrowseSource::Everyone && query.text_scope == TextScope::Any && !query.exact_phrase;
		let words: Vec<String> = text.split_whitespace().map(str::to_lowercase).collect();
		let words = if steam_matches {
			Vec::new()
		} else if query.exact_phrase && !words.is_empty() {
			vec![words.join(" ")]
		} else {
			words
		};
		Ok(Plan {
			kind: query.kind,
			source: query.source,
			author,
			sort: query.sort,
			trend_days: query.trend_days,
			text,
			text_scope: query.text_scope,
			words,
			types: clean_list(&query.types, "types")?,
			tags: clean_list(&query.tags, "tags")?,
			match_all: query.match_all,
			excluded_tags: clean_list(&query.excluded_tags, "excluded tags")?,
			created: query.created,
			updated: query.updated,
			size_min: query.size_min,
			size_max: query.size_max,
			min_subscribers: query.min_subscribers,
			min_rating: query.min_rating,
			status: query.status,
			exclude_ids,
		})
	}

	/// Steam cuts descriptions to 255 bytes unless a query asks for all of them.
	fn long_descriptions(&self) -> bool {
		!self.words.is_empty() && self.text_scope != TextScope::Title
	}

	fn user_list(&self) -> bool {
		matches!(self.source, BrowseSource::Author | BrowseSource::Favorites)
	}

	fn request(&self) -> Request {
		use sys::{EUGCQuery::*, EUserUGCList::*, EUserUGCListSortOrder::*};
		let order = match self.sort {
			BrowseSort::Updated => k_EUserUGCListSortOrder_LastUpdatedDesc,
			BrowseSort::TopRated => k_EUserUGCListSortOrder_VoteScoreDesc,
			_ => k_EUserUGCListSortOrder_CreationOrderDesc,
		};
		match self.source {
			BrowseSource::Author => Request::User {
				account: self.author.map_or(0, |author| author.account_id().raw()),
				list: k_EUserUGCList_Published,
				order,
			},
			BrowseSource::Favorites => Request::User {
				account: steam!().client().steam_id.account_id().raw(),
				list: k_EUserUGCList_Favorited,
				order,
			},
			BrowseSource::Friends => Request::All {
				query: k_EUGCQuery_CreatedByFriendsRankedByPublicationDate,
			},
			BrowseSource::Followed => Request::All {
				query: k_EUGCQuery_CreatedByFollowedUsersRankedByPublicationDate,
			},
			BrowseSource::Everyone => Request::All {
				query: match self.sort {
					BrowseSort::Popular => k_EUGCQuery_RankedByTrend,
					BrowseSort::TopRated => k_EUGCQuery_RankedByVote,
					BrowseSort::MostSubscribed => k_EUGCQuery_RankedByTotalUniqueSubscriptions,
					BrowseSort::Newest => k_EUGCQuery_RankedByPublicationDate,
					BrowseSort::Updated => k_EUGCQuery_RankedByLastUpdatedDate,
					BrowseSort::Relevance => k_EUGCQuery_RankedByTextSearch,
				},
			},
		}
	}

	/// Steam already applies most of these, but user lists ignore some options, so every item is checked.
	fn matches(&self, item: &ScannedItem, subscribed: &dyn Fn(PublishedFileId) -> bool) -> bool {
		let workshop = &item.workshop;
		let tagged = |name: &String| workshop.tags.iter().any(|tag| tag.eq_ignore_ascii_case(name));
		let kind = if item.collection { self.kind == BrowseKind::Collection } else { self.kind.tag().is_some_and(|tag| ItemKind::of(&workshop.tags) == ItemKind::of(&[tag.to_owned()])) };
		let tags = if self.match_all { self.tags.iter().all(tagged) } else { self.tags.is_empty() || self.tags.iter().any(tagged) };
		let size = workshop.file_size.unwrap_or(0);
		let description = || workshop.description.as_deref().unwrap_or_default();
		let haystack = || {
			match self.text_scope {
				TextScope::Any => format!("{}\n{}\n{}", workshop.title, description(), workshop.tags.join("\n")),
				TextScope::Title => workshop.title.clone(),
				TextScope::Description => description().to_owned(),
			}
			.to_lowercase()
		};
		kind && !self.exclude_ids.contains(&workshop.id)
			&& (self.types.is_empty() || self.types.iter().any(tagged))
			&& tags
			&& !self.excluded_tags.iter().any(tagged)
			&& self.created.is_none_or(|range| range.contains(workshop.time_created))
			&& self.updated.is_none_or(|range| range.contains(workshop.time_updated))
			&& self.size_min.is_none_or(|min| size >= min)
			&& self.size_max.is_none_or(|max| size <= max)
			&& self.min_subscribers.is_none_or(|min| workshop.subscriptions >= min)
			&& self.min_rating.is_none_or(|min| workshop.score >= min)
			&& match self.status {
				BrowseStatus::Any => true,
				BrowseStatus::Subscribed => subscribed(workshop.id),
				BrowseStatus::NotSubscribed => !subscribed(workshop.id),
			} && (self.words.is_empty() || {
			let haystack = haystack();
			self.words.iter().all(|word| haystack.contains(word.as_str()))
		})
	}
}

struct ScannedItem {
	workshop: WorkshopItem,
	collection: bool,
	gmod: bool,
	children: Vec<PublishedFileId>,
	previews: Vec<Preview>,
}

struct ScannedPage {
	items: Vec<ScannedItem>,
	returned: u32,
	total: u32,
	next: Option<Position>,
}

struct QueryHandle {
	ugc: *mut sys::ISteamUGC,
	handle: sys::UGCQueryHandle_t,
}

impl QueryHandle {
	fn option(&self, accepted: bool, what: &str) -> Result<(), String> {
		if accepted {
			Ok(())
		} else {
			Err(format!("Steam did not accept the {what} for this search."))
		}
	}

	/// Items must have at least one of these tags; separate groups and required tags must all match.
	fn tag_group(&self, tags: &[CString], what: &str) -> Result<(), String> {
		let mut pointers: Vec<*const c_char> = tags.iter().map(|tag| tag.as_ptr()).collect();
		let group = sys::SteamParamStringArray_t {
			m_ppStrings: pointers.as_mut_ptr(),
			m_nNumStrings: pointers.len() as i32,
		};
		self.option(unsafe { sys::SteamAPI_ISteamUGC_AddRequiredTagGroup(self.ugc, self.handle, &group) }, what)
	}
}

impl Drop for QueryHandle {
	fn drop(&mut self) {
		unsafe {
			sys::SteamAPI_ISteamUGC_ReleaseQueryUGCRequest(self.ugc, self.handle);
		}
	}
}

fn c_string(value: &str) -> Result<CString, String> {
	CString::new(value).map_err(|_| "Text sent to Steam cannot contain null characters.".to_string())
}

fn read_text(buffer: &[c_char]) -> String {
	if !buffer.contains(&0) {
		return String::new();
	}
	unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy().into_owned()
}

/// Waits for a raw call result while holding the callback dispatch lock, as `change_dependency` does,
/// because steamworks' callback pump drops call results it did not register.
unsafe fn await_call<T>(utils: *mut sys::ISteamUtils, call: sys::SteamAPICall_t, callback: i32, timeout: &str) -> Result<T, String> {
	if call == sys::k_uAPICallInvalid {
		return Err("Steam did not start the request. Try again.".into());
	}
	let start = Instant::now();
	let mut failed = false;
	while !sys::SteamAPI_ISteamUtils_IsAPICallCompleted(utils, call, &mut failed) {
		if start.elapsed() >= STEAM_TIMEOUT {
			return Err(timeout.into());
		}
		sys::SteamAPI_ManualDispatch_RunFrame(sys::SteamAPI_GetHSteamPipe());
		std::thread::sleep(Duration::from_millis(20));
	}
	if failed {
		return Err(format!("Steam connection failure: {:?}", sys::SteamAPI_ISteamUtils_GetAPICallFailureReason(utils, call)));
	}
	let mut result = MaybeUninit::<T>::uninit();
	if !sys::SteamAPI_ISteamUtils_GetAPICallResult(utils, call, result.as_mut_ptr().cast(), std::mem::size_of::<T>() as i32, callback, &mut failed) || failed {
		return Err("Could not read Steam's response. Try again.".into());
	}
	Ok(result.assume_init())
}

fn with_interfaces<T>(run: impl FnOnce(*mut sys::ISteamUGC, *mut sys::ISteamUtils) -> Result<T, String>) -> Result<T, String> {
	connected()?;
	let _dispatch = steam!().callback_dispatch.lock();
	let _client = steam!().client();
	let (ugc, utils) = unsafe { (sys::SteamAPI_SteamUGC_v021(), sys::SteamAPI_SteamUtils_v010()) };
	if ugc.is_null() || utils.is_null() {
		return Err("Steam Workshop is unavailable.".into());
	}
	run(ugc, utils)
}

/// Sends one query and reads its results; the query handle is always released.
fn run_query<T>(
	create: impl FnOnce(*mut sys::ISteamUGC) -> sys::UGCQueryHandle_t,
	configure: impl FnOnce(&QueryHandle) -> Result<(), String>,
	read: impl FnOnce(&QueryHandle, &sys::SteamUGCQueryCompleted_t) -> Result<T, String>,
) -> Result<T, String> {
	with_interfaces(|ugc, utils| {
		let handle = create(ugc);
		if handle == sys::k_UGCQueryHandleInvalid {
			return Err("Steam could not start the Workshop search.".into());
		}
		let query = QueryHandle { ugc, handle };
		configure(&query)?;
		let completed: sys::SteamUGCQueryCompleted_t = unsafe {
			let call = sys::SteamAPI_ISteamUGC_SendQueryUGCRequest(ugc, handle);
			await_call(utils, call, sys::SteamUGCQueryCompleted_t_k_iCallback as i32, "Steam took too long to answer the Workshop search. Try again.")?
		};
		if completed.m_handle != handle {
			return Err("Steam returned results for a different search. Try again.".into());
		}
		if completed.m_eResult != sys::EResult::k_EResultOK {
			return Err(format!("Workshop search failed: {}", SteamError::from(completed.m_eResult)));
		}
		read(&query, &completed)
	})
}

fn statistic(query: &QueryHandle, index: u32, stat: sys::EItemStatistic) -> Option<u64> {
	let mut value = 0;
	unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCStatistic(query.ugc, query.handle, index, stat, &mut value) }.then_some(value)
}

fn youtube_id(id: &str) -> bool {
	!id.is_empty() && id.len() <= 32 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn read_previews(query: &QueryHandle, index: u32) -> Vec<Preview> {
	let count = unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCNumAdditionalPreviews(query.ugc, query.handle, index) };
	let mut previews = Vec::new();
	for preview in 0..count {
		let mut url = [0 as c_char; 1024];
		let mut file_name = [0 as c_char; 260];
		let mut kind = sys::EItemPreviewType::k_EItemPreviewType_ReservedMax;
		let ok = unsafe {
			sys::SteamAPI_ISteamUGC_GetQueryUGCAdditionalPreview(
				query.ugc,
				query.handle,
				index,
				preview,
				url.as_mut_ptr(),
				url.len() as u32,
				file_name.as_mut_ptr(),
				file_name.len() as u32,
				&mut kind,
			)
		};
		let url = read_text(&url);
		if !ok || url.is_empty() {
			continue;
		}
		match kind {
			sys::EItemPreviewType::k_EItemPreviewType_Image if url.starts_with("https://") => previews.push(Preview::Image(url)),
			sys::EItemPreviewType::k_EItemPreviewType_YouTubeVideo if youtube_id(&url) => previews.push(Preview::Youtube(url)),
			_ => {}
		}
	}
	previews
}

fn read_item(query: &QueryHandle, index: u32, children: bool, previews: bool) -> Result<Option<ScannedItem>, String> {
	let mut details: sys::SteamUGCDetails_t = unsafe { std::mem::zeroed() };
	if !unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCResult(query.ugc, query.handle, index, &mut details) } {
		return Err("Steam returned an incomplete page of results. Try again.".into());
	}
	if details.m_eResult != sys::EResult::k_EResultOK {
		return Ok(None);
	}
	let id = PublishedFileId(details.m_nPublishedFileId);
	let collection = details.m_eFileType == sys::EWorkshopFileType::k_EWorkshopFileTypeCollection;
	let tags: Vec<String> = read_text(&details.m_rgchTags).split(',').filter(|tag| !tag.is_empty()).map(str::to_owned).collect();
	let mut preview_url = [0 as c_char; 4096];
	let preview_url = unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCPreviewURL(query.ugc, query.handle, index, preview_url.as_mut_ptr(), preview_url.len() as u32) }
		.then(|| read_text(&preview_url))
		.filter(|url| !url.is_empty());
	let size = if details.m_ulTotalFilesSize > 0 { details.m_ulTotalFilesSize } else { details.m_nFileSize.max(0) as u64 };
	let children = if children && details.m_unNumChildren > 0 {
		let mut ids = vec![0; details.m_unNumChildren as usize];
		if !unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCChildren(query.ugc, query.handle, index, ids.as_mut_ptr(), ids.len() as u32) } {
			return Err(format!("Could not read the items of {}.", id.0));
		}
		ids.into_iter().map(PublishedFileId).collect()
	} else {
		Vec::new()
	};
	use sys::EItemStatistic::*;
	let workshop = WorkshopItem {
		id,
		title: read_text(&details.m_rgchTitle),
		owner: None,
		time_created: details.m_rtimeCreated,
		time_updated: details.m_rtimeUpdated,
		visibility: Some(WorkshopVisibility::from(steamworks::PublishedFileVisibility::from(details.m_eVisibility))),
		description: Some(read_text(&details.m_rgchDescription)),
		score: details.m_flScore,
		kind: Some(if collection { ItemKind::Collection } else { ItemKind::of(&tags) }),
		tags,
		preview_url,
		subscriptions: statistic(query, index, k_EItemStatistic_NumSubscriptions).unwrap_or(0),
		banned: Some(details.m_bBanned),
		file_size: (size > 0).then_some(size),
		file_name: original_file_name(&read_text(&details.m_pchFileName)),
		upvotes: Some(details.m_unVotesUp),
		downvotes: Some(details.m_unVotesDown),
		favorites: statistic(query, index, k_EItemStatistic_NumFavorites),
		views: statistic(query, index, k_EItemStatistic_NumUniqueWebsiteViews),
		comments: statistic(query, index, k_EItemStatistic_NumComments),
		num_children: Some(details.m_unNumChildren),
		local_file: None,
		steamid: Some(SteamId::from_raw(details.m_ulSteamIDOwner)),
		dead: false,
	};
	Ok(Some(ScannedItem {
		workshop,
		collection,
		gmod: details.m_nConsumerAppID == GMOD_APP_ID.0,
		children,
		previews: if previews { read_previews(query, index) } else { Vec::new() },
	}))
}

fn read_page(query: &QueryHandle, completed: &sys::SteamUGCQueryCompleted_t) -> Result<Vec<ScannedItem>, String> {
	let mut items = Vec::with_capacity(completed.m_unNumResultsReturned as usize);
	for index in 0..completed.m_unNumResultsReturned {
		items.extend(read_item(query, index, false, false)?);
	}
	Ok(items)
}

fn fetch_page(plan: &Plan, position: &Position) -> Result<ScannedPage, String> {
	let request = plan.request();
	let user_list = matches!(request, Request::User { .. });
	let cursor = match position {
		Position::Cursor(cursor) => Some(c_string(cursor)?),
		Position::Page(_) => None,
	};
	let page = match position {
		Position::Page(page) => *page,
		Position::Cursor(_) => 0,
	};
	let text = c_string(&plan.text)?;
	let tags = plan.tags.iter().map(|tag| c_string(tag)).collect::<Result<Vec<_>, _>>()?;
	let excluded = plan.excluded_tags.iter().map(|tag| c_string(tag)).collect::<Result<Vec<_>, _>>()?;
	let types = plan.types.iter().map(|tag| c_string(tag)).collect::<Result<Vec<_>, _>>()?;
	let kind = plan.kind.tag().map(c_string).transpose()?;
	run_query(
		|ugc| unsafe {
			match request {
				Request::All { query } => sys::SteamAPI_ISteamUGC_CreateQueryAllUGCRequestCursor(ugc, query, plan.kind.matching(), 0, GMOD_APP_ID.0, cursor.as_ref().map_or(std::ptr::null(), |cursor| cursor.as_ptr())),
				Request::User { account, list, order } => sys::SteamAPI_ISteamUGC_CreateQueryUserUGCRequest(ugc, account, list, plan.kind.matching(), order, 0, GMOD_APP_ID.0, page),
			}
		},
		|query| unsafe {
			let (ugc, handle) = (query.ugc, query.handle);
			query.option(sys::SteamAPI_ISteamUGC_SetAllowCachedResponse(ugc, handle, 60), "cache setting")?;
			if plan.long_descriptions() {
				query.option(sys::SteamAPI_ISteamUGC_SetReturnLongDescription(ugc, handle, true), "description request")?;
			}
			if let Some(kind) = &kind {
				query.option(sys::SteamAPI_ISteamUGC_AddRequiredTag(ugc, handle, kind.as_ptr()), "item type")?;
			}
			if !types.is_empty() {
				query.tag_group(&types, "type filter")?;
			}
			if plan.match_all {
				for tag in &tags {
					query.option(sys::SteamAPI_ISteamUGC_AddRequiredTag(ugc, handle, tag.as_ptr()), "tag filter")?;
				}
			} else if !tags.is_empty() && !user_list {
				query.tag_group(&tags, "tag filter")?;
			}
			for tag in &excluded {
				query.option(sys::SteamAPI_ISteamUGC_AddExcludedTag(ugc, handle, tag.as_ptr()), "excluded tags")?;
			}
			if !user_list {
				if plan.source == BrowseSource::Everyone && !plan.text.is_empty() {
					query.option(sys::SteamAPI_ISteamUGC_SetSearchText(ugc, handle, text.as_ptr()), "search text")?;
				}
				if plan.sort == BrowseSort::Popular {
					query.option(sys::SteamAPI_ISteamUGC_SetRankedByTrendDays(ugc, handle, plan.trend_days), "popular period")?;
				}
				if let Some(range) = plan.created {
					let (start, end) = range.bounds();
					query.option(sys::SteamAPI_ISteamUGC_SetTimeCreatedDateRange(ugc, handle, start, end), "upload dates")?;
				}
				if let Some(range) = plan.updated {
					let (start, end) = range.bounds();
					query.option(sys::SteamAPI_ISteamUGC_SetTimeUpdatedDateRange(ugc, handle, start, end), "update dates")?;
				}
			}
			Ok(())
		},
		|query, completed| {
			let items = read_page(query, completed)?;
			let returned = completed.m_unNumResultsReturned;
			let next = match position {
				Position::Page(page) => ((*page as u64) * (RESULTS_PER_PAGE as u64) < completed.m_unTotalMatchingResults as u64 && returned > 0).then(|| Position::Page(page + 1)),
				Position::Cursor(current) => {
					let next = read_text(&completed.m_rgchNextCursor);
					(returned > 0 && !next.is_empty() && next != *current).then_some(Position::Cursor(next))
				}
			};
			Ok(ScannedPage {
				items,
				returned,
				total: completed.m_unTotalMatchingResults,
				next,
			})
		},
	)
}

fn is_subscribed(id: PublishedFileId, unsubscribed: &HashSet<PublishedFileId>, subscribed: &HashSet<PublishedFileId>) -> bool {
	if unsubscribed.contains(&id) {
		return false;
	}
	subscribed.contains(&id) || steam!().client().ugc().item_state(id).contains(ItemState::SUBSCRIBED)
}

fn scan(plan: Plan, cursor: Option<String>, request: u64) -> Result<BrowsePage, String> {
	connected()?;
	let superseded = || LATEST_REQUEST.load(Ordering::Acquire) > request;
	if cursor.is_none() {
		LATEST_REQUEST.fetch_max(request, Ordering::AcqRel);
	}
	let mut position = Position::decode(plan.user_list(), cursor.as_deref())?;
	let (unsubscribed, subscribed) = game_addons!().session_subscriptions();
	let subscribed = |id| is_subscribed(id, &unsubscribed, &subscribed);
	let mut seen = HashSet::new();
	let mut result = BrowsePage {
		total: 0,
		scanned: 0,
		items: Vec::new(),
		next_cursor: None,
		more: false,
	};
	for _ in 0..SCAN_PAGES {
		if superseded() {
			return Err(SUPERSEDED.into());
		}
		let page = fetch_page(&plan, &position)?;
		result.total = page.total;
		result.scanned += page.returned;
		for item in page.items {
			if seen.insert(item.workshop.id) && plan.matches(&item, &subscribed) {
				let mut workshop = item.workshop;
				workshop.description = None;
				result.items.push(workshop.into());
			}
		}
		let Some(next) = page.next else {
			result.more = false;
			result.next_cursor = None;
			return Ok(result);
		};
		result.more = true;
		result.next_cursor = Some(next.encode());
		position = next;
		if result.items.len() >= PAGE_TARGET {
			break;
		}
	}
	Ok(result)
}

fn query_details(ids: &[PublishedFileId], long: bool) -> Result<Vec<ScannedItem>, String> {
	let mut ids: Vec<u64> = ids.iter().map(|id| id.0).collect();
	run_query(
		|ugc| unsafe { sys::SteamAPI_ISteamUGC_CreateQueryUGCDetailsRequest(ugc, ids.as_mut_ptr(), ids.len() as u32) },
		|query| unsafe {
			query.option(sys::SteamAPI_ISteamUGC_SetAllowCachedResponse(query.ugc, query.handle, 0), "cache setting")?;
			if long {
				query.option(sys::SteamAPI_ISteamUGC_SetReturnLongDescription(query.ugc, query.handle, true), "description request")?;
				query.option(sys::SteamAPI_ISteamUGC_SetReturnChildren(query.ugc, query.handle, true), "children request")?;
				query.option(sys::SteamAPI_ISteamUGC_SetReturnAdditionalPreviews(query.ugc, query.handle, true), "preview request")?;
			}
			Ok(())
		},
		|query, completed| {
			let mut items = Vec::new();
			for index in 0..completed.m_unNumResultsReturned {
				items.extend(read_item(query, index, long, long)?);
			}
			Ok(items)
		},
	)
}

fn my_vote(id: PublishedFileId) -> Result<Option<Vote>, String> {
	with_interfaces(|ugc, utils| unsafe {
		let call = sys::SteamAPI_ISteamUGC_GetUserItemVote(ugc, id.0);
		let result: sys::GetUserItemVoteResult_t = await_call(utils, call, sys::GetUserItemVoteResult_t_k_iCallback as i32, "Steam took too long to return your rating.")?;
		if result.m_nPublishedFileId != id.0 {
			return Err("Steam returned a rating for a different item.".into());
		}
		if result.m_eResult != sys::EResult::k_EResultOK {
			return Err(format!("Could not load your rating: {}", SteamError::from(result.m_eResult)));
		}
		Ok(if result.m_bVotedUp {
			Some(Vote::Up)
		} else if result.m_bVotedDown {
			Some(Vote::Down)
		} else {
			None
		})
	})
}

fn details(id: PublishedFileId) -> Result<BrowseDetails, String> {
	let item = query_details(&[id], true)?
		.into_iter()
		.find(|item| item.workshop.id == id)
		.ok_or_else(|| format!("Workshop item {} is unavailable. It may be private or deleted.", id.0))?;
	if !item.gmod {
		return Err("This item is not for Garry's Mod.".into());
	}
	let mut details = BrowseDetails {
		workshop: item.workshop,
		previews: item.previews,
		dependencies: Vec::new(),
		children: Vec::new(),
		total_size: None,
		vote: my_vote(id)?,
	};
	if item.collection {
		let mut total = 0;
		for chunk in item.children.chunks(RESULTS_PER_PAGE) {
			let found = query_details(chunk, false)?;
			for child in chunk {
				match found.iter().find(|item| item.workshop.id == *child) {
					Some(item) => {
						total += item.workshop.file_size.unwrap_or(0);
						let mut workshop = item.workshop.clone();
						workshop.description = None;
						details.children.push(workshop.into());
					}
					None => details.children.push(WorkshopItem::from(*child).into()),
				}
			}
		}
		details.total_size = Some(total);
	} else if !item.children.is_empty() {
		details.dependencies = lookup_entries(item.children.iter().map(|id| id.0.to_string()).collect())?.entries;
	}
	Ok(details)
}

fn load_favorites() -> Result<HashSet<PublishedFileId>, String> {
	let account = steam!().client().steam_id;
	if let Some((owner, ids)) = FAVORITES.lock().as_ref() {
		if *owner == account {
			return Ok(ids.clone());
		}
	}
	let mut ids = HashSet::new();
	for page in 1u32.. {
		let (returned, total) = run_query(
			|ugc| unsafe {
				sys::SteamAPI_ISteamUGC_CreateQueryUserUGCRequest(
					ugc,
					account.account_id().raw(),
					sys::EUserUGCList::k_EUserUGCList_Favorited,
					sys::EUGCMatchingUGCType::k_EUGCMatchingUGCType_All,
					sys::EUserUGCListSortOrder::k_EUserUGCListSortOrder_CreationOrderDesc,
					0,
					GMOD_APP_ID.0,
					page,
				)
			},
			|query| unsafe {
				query.option(sys::SteamAPI_ISteamUGC_SetReturnOnlyIDs(query.ugc, query.handle, true), "ID request")?;
				query.option(sys::SteamAPI_ISteamUGC_SetAllowCachedResponse(query.ugc, query.handle, 0), "cache setting")
			},
			|query, completed| {
				for index in 0..completed.m_unNumResultsReturned {
					let mut details: sys::SteamUGCDetails_t = unsafe { std::mem::zeroed() };
					if unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCResult(query.ugc, query.handle, index, &mut details) } && details.m_nPublishedFileId != 0 {
						ids.insert(PublishedFileId(details.m_nPublishedFileId));
					}
				}
				Ok((completed.m_unNumResultsReturned, completed.m_unTotalMatchingResults))
			},
		)?;
		if returned == 0 || page as u64 * RESULTS_PER_PAGE as u64 >= total as u64 {
			break;
		}
	}
	if steam!().client().steam_id != account {
		return Err("The Steam account changed. Try again.".into());
	}
	*FAVORITES.lock() = Some((account, ids.clone()));
	Ok(ids)
}

fn set_vote(id: PublishedFileId, up: bool) -> Result<(), String> {
	with_interfaces(|ugc, utils| unsafe {
		let call = sys::SteamAPI_ISteamUGC_SetUserItemVote(ugc, id.0, up);
		let result: sys::SetUserItemVoteResult_t = await_call(utils, call, sys::SetUserItemVoteResult_t_k_iCallback as i32, "Steam took too long to save your rating. It may still be saved.")?;
		if result.m_nPublishedFileId != id.0 {
			return Err("Steam returned a rating for a different item.".into());
		}
		if result.m_eResult != sys::EResult::k_EResultOK {
			return Err(format!("Could not save your rating: {}", SteamError::from(result.m_eResult)));
		}
		Ok(())
	})
}

fn set_favorite(id: PublishedFileId, favorite: bool) -> Result<(), String> {
	let account = steam!().client().steam_id;
	with_interfaces(|ugc, utils| unsafe {
		let call = if favorite {
			sys::SteamAPI_ISteamUGC_AddItemToFavorites(ugc, GMOD_APP_ID.0, id.0)
		} else {
			sys::SteamAPI_ISteamUGC_RemoveItemFromFavorites(ugc, GMOD_APP_ID.0, id.0)
		};
		let result: sys::UserFavoriteItemsListChanged_t =
			await_call(utils, call, sys::UserFavoriteItemsListChanged_t_k_iCallback as i32, "Steam took too long to change your favorites. The change may still be saved.")?;
		if result.m_nPublishedFileId != id.0 || result.m_bWasAddRequest != favorite {
			return Err("Steam returned a different favorites change.".into());
		}
		if result.m_eResult != sys::EResult::k_EResultOK {
			return Err(format!("Could not change your favorites: {}", SteamError::from(result.m_eResult)));
		}
		Ok(())
	})?;
	if let Some((owner, ids)) = FAVORITES.lock().as_mut() {
		if *owner == account {
			if favorite {
				ids.insert(id);
			} else {
				ids.remove(&id);
			}
		}
	}
	Ok(())
}

fn reference(value: &str) -> Result<PublishedFileId, String> {
	parse_reference(value)?.ok_or_else(|| "Enter a Workshop link or item ID.".into())
}

async fn blocking<T: Send + 'static>(task: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
	tauri::async_runtime::spawn_blocking(task).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn browse_workshop(query: BrowseQuery, cursor: Option<String>, request: u64) -> Result<BrowsePage, String> {
	let plan = Plan::new(query)?;
	let result = blocking(move || scan(plan, cursor, request)).await;
	if let Err(error) = &result {
		if error != SUPERSEDED {
			eprintln!("Could not browse the Workshop: {}", error);
		}
	}
	result
}

#[tauri::command]
pub async fn browse_item(reference: String) -> Result<BrowseDetails, String> {
	let id = self::reference(&reference)?;
	let result = blocking(move || details(id)).await;
	if let Err(error) = &result {
		eprintln!("Could not load Workshop item {}: {}", id.0, error);
	}
	result
}

#[tauri::command]
pub async fn set_workshop_vote(id: String, up: bool) -> Result<(), String> {
	let id = reference(&id)?;
	let result = blocking(move || set_vote(id, up)).await;
	match &result {
		Ok(()) => println!("Rated Workshop item {} {}", id.0, if up { "up" } else { "down" }),
		Err(error) => eprintln!("Could not rate Workshop item {}: {}", id.0, error),
	}
	result
}

#[tauri::command]
pub async fn set_workshop_favorite(id: String, favorite: bool) -> Result<(), String> {
	let id = reference(&id)?;
	let result = blocking(move || set_favorite(id, favorite)).await;
	match &result {
		Ok(()) => println!("{} Workshop item {} {} favorites", if favorite { "Added" } else { "Removed" }, id.0, if favorite { "to" } else { "from" }),
		Err(error) => eprintln!("Could not change favorite Workshop item {}: {}", id.0, error),
	}
	result
}

#[tauri::command]
pub async fn my_favorite_ids() -> Result<Vec<String>, String> {
	blocking(|| Ok(load_favorites()?.into_iter().map(|id| id.0.to_string()).collect())).await
}

#[tauri::command]
pub async fn get_steam_users(ids: Vec<String>) -> Result<Vec<SteamUser>, String> {
	if ids.len() > USERS_MAX {
		return Err(format!("Load at most {USERS_MAX} Steam users at once."));
	}
	let ids = ids
		.iter()
		.map(|id| id.parse::<u64>().ok().filter(|id| *id != 0).map(SteamId::from_raw).ok_or_else(|| format!("{id:?} is not a Steam ID.")))
		.collect::<Result<HashSet<_>, _>>()?;
	blocking(move || {
		connected()?;
		Ok(steam!().fetch_users(ids.into_iter().collect()))
	})
	.await
}

#[cfg(test)]
mod tests {
	use super::*;

	fn query(value: serde_json::Value) -> Result<Plan, String> {
		Plan::new(serde_json::from_value(value).map_err(|error| error.to_string())?)
	}

	fn item(tags: &[&str], collection: bool) -> ScannedItem {
		let mut workshop = WorkshopItem::from(PublishedFileId(10));
		workshop.dead = false;
		workshop.title = "Wire Tools".into();
		workshop.tags = tags.iter().map(|tag| tag.to_string()).collect();
		workshop.time_created = 1_000;
		workshop.time_updated = 2_000;
		workshop.file_size = Some(5_000);
		workshop.subscriptions = 300;
		workshop.score = 0.8;
		ScannedItem {
			workshop,
			collection,
			gmod: true,
			children: Vec::new(),
			previews: Vec::new(),
		}
	}

	fn matches(plan: &Plan, item: &ScannedItem) -> bool {
		plan.matches(item, &|id| id.0 == 10)
	}

	#[test]
	fn queries_accept_defaults_and_reject_invalid_combinations() {
		let plan = query(serde_json::json!({"kind": "addon", "source": "everyone", "sort": "popular"})).unwrap();
		assert_eq!(plan.trend_days, 7);
		assert!(plan.words.is_empty());
		for invalid in [
			serde_json::json!({"kind": "addon", "source": "everyone", "sort": "relevance"}),
			serde_json::json!({"kind": "addon", "source": "everyone", "sort": "popular", "trendDays": 0}),
			serde_json::json!({"kind": "addon", "source": "everyone", "sort": "popular", "trendDays": 366}),
			serde_json::json!({"kind": "addon", "source": "author", "sort": "newest"}),
			serde_json::json!({"kind": "addon", "source": "author", "author": "0", "sort": "newest"}),
			serde_json::json!({"kind": "addon", "source": "everyone", "author": "76561197960287930", "sort": "newest"}),
			serde_json::json!({"kind": "addon", "source": "friends", "sort": "popular"}),
			serde_json::json!({"kind": "addon", "source": "favorites", "sort": "most_subscribed"}),
			serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest", "created": {"start": 5, "end": 4}}),
			serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest", "sizeMin": 5, "sizeMax": 4}),
			serde_json::json!({"kind": "collection", "source": "everyone", "sort": "newest", "sizeMin": 5}),
			serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest", "minRating": 1.5}),
			serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest", "tags": ["a,b"]}),
			serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest", "tags": [""]}),
			serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest", "excludeIds": ["abc"]}),
			serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest", "text": "a".repeat(257)}),
			serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest", "unknown": true}),
			serde_json::json!({"kind": "map", "source": "everyone", "sort": "newest"}),
		] {
			assert!(query(invalid.clone()).is_err(), "{invalid}");
		}
		let plan = query(serde_json::json!({"kind": "addon", "source": "author", "author": "76561197960287930", "sort": "updated", "text": " Wire  TOOLS "})).unwrap();
		assert_eq!(plan.author, Some(SteamId::from_raw(76561197960287930)));
		assert_eq!(plan.words, ["wire", "tools"]);
	}

	#[test]
	fn tag_lists_are_trimmed_and_deduplicated_without_changing_case() {
		let plan = query(serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest", "tags": [" Fun ", "fun", "Build"]})).unwrap();
		assert_eq!(plan.tags, ["Fun", "Build"]);
	}

	#[test]
	fn cursors_round_trip_and_reject_mismatched_sources() {
		assert_eq!(Position::decode(false, None).unwrap(), Position::Cursor("*".into()));
		assert_eq!(Position::decode(true, None).unwrap(), Position::Page(1));
		let cursor = Position::Cursor("AoJwq5vz3fwC".into());
		assert_eq!(Position::decode(false, Some(&cursor.encode())).unwrap(), cursor);
		assert_eq!(Position::decode(true, Some(&Position::Page(4).encode())).unwrap(), Position::Page(4));
		for (user_list, cursor) in [(true, "AoJwq5vz3fwC"), (true, "page:0"), (true, "page:x"), (false, "page:2"), (false, "")] {
			assert!(Position::decode(user_list, Some(cursor)).is_err(), "{cursor}");
		}
	}

	#[test]
	fn in_app_filters_check_kind_tags_dates_size_rating_and_status() {
		let addon = item(&["Addon", "tool", "Fun"], false);
		let base = serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest"});
		let with = |extra: serde_json::Value| {
			let mut value = base.clone();
			value.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
			query(value).unwrap()
		};
		assert!(matches(&with(serde_json::json!({})), &addon));
		assert!(!matches(&with(serde_json::json!({"kind": "dupe"})), &addon));
		assert!(!matches(&with(serde_json::json!({"kind": "collection", "sort": "newest"})), &addon));
		assert!(matches(&with(serde_json::json!({"kind": "collection"})), &item(&[], true)));
		assert!(matches(&with(serde_json::json!({"types": ["Weapon", "TOOL"]})), &addon));
		assert!(!matches(&with(serde_json::json!({"types": ["map"]})), &addon));
		assert!(matches(&with(serde_json::json!({"tags": ["fun", "build"]})), &addon));
		assert!(!matches(&with(serde_json::json!({"tags": ["fun", "build"], "matchAll": true})), &addon));
		assert!(!matches(&with(serde_json::json!({"excludedTags": ["FUN"]})), &addon));
		assert!(matches(&with(serde_json::json!({"created": {"start": 1000, "end": 1000}, "updated": {"start": 1500}})), &addon));
		assert!(!matches(&with(serde_json::json!({"updated": {"end": 1999}})), &addon));
		assert!(matches(&with(serde_json::json!({"sizeMin": 5000, "sizeMax": 5000})), &addon));
		assert!(!matches(&with(serde_json::json!({"sizeMin": 5001})), &addon));
		assert!(!matches(&with(serde_json::json!({"minSubscribers": 301})), &addon));
		assert!(!matches(&with(serde_json::json!({"minRating": 0.9})), &addon));
		assert!(matches(&with(serde_json::json!({"status": "subscribed"})), &addon));
		assert!(!matches(&with(serde_json::json!({"status": "not_subscribed"})), &addon));
		assert!(!matches(&with(serde_json::json!({"excludeIds": ["10"]})), &addon));
	}

	#[test]
	fn text_is_matched_in_app_only_for_sources_steam_cannot_search() {
		let addon = item(&["Addon", "tool"], false);
		let everyone = query(serde_json::json!({"kind": "addon", "source": "everyone", "sort": "relevance", "text": "something else"})).unwrap();
		assert!(matches(&everyone, &addon));
		let favorites = query(serde_json::json!({"kind": "addon", "source": "favorites", "sort": "newest", "text": "wire TOOL"})).unwrap();
		assert!(matches(&favorites, &addon));
		let favorites = query(serde_json::json!({"kind": "addon", "source": "favorites", "sort": "newest", "text": "wire lamp"})).unwrap();
		assert!(!matches(&favorites, &addon));
	}

	#[test]
	fn text_scope_and_exact_phrase_are_matched_in_app() {
		let mut addon = item(&["Addon", "tool"], false);
		addon.workshop.description = Some("Adds a lamp and wire gates.".into());
		let with = |extra: serde_json::Value| {
			let mut value = serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest"});
			value.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
			query(value).unwrap()
		};
		let title = with(serde_json::json!({"text": "tools WIRE", "textScope": "title"}));
		assert!(matches(&title, &addon));
		assert!(!title.long_descriptions());
		assert!(!matches(&with(serde_json::json!({"text": "lamp", "textScope": "title"})), &addon));
		let description = with(serde_json::json!({"text": "gates lamp", "textScope": "description"}));
		assert!(matches(&description, &addon));
		assert!(description.long_descriptions());
		assert!(!matches(&with(serde_json::json!({"text": "tools", "textScope": "description"})), &addon));
		let phrase = with(serde_json::json!({"text": " Wire  Tools ", "exactPhrase": true}));
		assert_eq!(phrase.words, ["wire tools"]);
		assert!(matches(&phrase, &addon));
		assert!(!matches(&with(serde_json::json!({"text": "tools wire", "exactPhrase": true})), &addon));
		assert!(with(serde_json::json!({"text": "", "exactPhrase": true, "textScope": "title"})).words.is_empty());
		assert!(query(serde_json::json!({"kind": "addon", "source": "everyone", "sort": "newest", "textScope": "tags"})).is_err());
	}

	#[test]
	fn youtube_previews_need_plain_video_ids() {
		assert!(youtube_id("dQw4w9WgXcQ"));
		for id in ["", "https://youtube.com/watch?v=1", "a/b", "a\"b"] {
			assert!(!youtube_id(id), "{id}");
		}
	}
}
