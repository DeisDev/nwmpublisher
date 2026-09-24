pub(crate) mod snapshots;
pub use snapshots::{transaction_snapshot, transaction_snapshots, transaction_data, acknowledge_transaction};

use lazy_static::lazy_static;
use parking_lot::{Mutex, RwLock};
use rayon::ThreadPool;
use serde::Serialize;
use std::sync::{
	atomic::{AtomicU32, Ordering},
	Arc, Weak,
};

use self::snapshots::TransactionMessage;

lazy_static! {
	static ref TRANSACTIONS: Transactions = Transactions::init();
	static ref TRANSACTIONS_SLAVE: ThreadPool = thread_pool!(1);
}

pub struct Transactions {
	inner: RwLock<Vec<TransactionRef>>,
	id: AtomicU32,
}
impl std::ops::Deref for Transactions {
	type Target = RwLock<Vec<TransactionRef>>;
	fn deref(&self) -> &Self::Target {
		&self.inner
	}
}
impl Transactions {
	fn init() -> Transactions {
		Transactions {
			inner: RwLock::new(Vec::new()),
			id: AtomicU32::new(0),
		}
	}

	pub fn find(&self, transaction_id: u32) -> Option<Transaction> {
		let transactions = self.inner.read();
		if let Ok(pos) = transactions.binary_search_by_key(&transaction_id, |transaction| transaction.id) {
			// A completed job can be dropped before its queued registry removal runs.
			return transactions[pos].upgrade();
		}

		None
	}
}

pub struct TransactionRef {
	pub id: u32,
	ptr: Weak<TransactionInner>,
}
impl std::ops::Deref for TransactionRef {
	type Target = Weak<TransactionInner>;
	fn deref(&self) -> &Self::Target {
		&self.ptr
	}
}
impl PartialOrd for TransactionRef {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		Some(self.cmp(other))
	}
}
impl Ord for TransactionRef {
	fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		self.id.cmp(&other.id)
	}
}
impl PartialEq for TransactionRef {
	fn eq(&self, other: &Self) -> bool {
		self.id == other.id
	}
}
impl Eq for TransactionRef {}

#[inline(always)]
fn progress_as_int(progress: f64) -> u16 {
	u16::min((progress * 10000.) as u16, 10000)
}

pub type Transaction = Arc<TransactionInner>;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
	Running,
	Preparing,
	Packing,
	Cancelling,
	Submitting,
	Unknown,
	Committing,
	Finished,
	Failed,
	Cancelled,
}
impl JobState {
	fn terminal(self) -> bool {
		matches!(self, Self::Finished | Self::Failed | Self::Cancelled)
	}
}

#[derive(Debug)]
pub struct TransactionInner {
	pub id: u32,
	state: Mutex<JobState>,
	cooperative: bool,
}
impl TransactionInner {
	pub fn warning(&self, message: String) {
		if *crate::cli::CLI_MODE { eprintln!("{}", message); return; }
		self.emit(TransactionMessage::Warning(self.id, message));
	}
	pub fn context(&self, context: serde_json::Value) {
		snapshots::context(self.id, context);
	}
	fn emit(&self, message: TransactionMessage) {
		if *crate::cli::CLI_MODE {
			return;
		}

		snapshots::record(message);
	}

	fn remove(&self) {
		let id = self.id;
		TRANSACTIONS_SLAVE.spawn(move || {
			let mut transactions = TRANSACTIONS.write();
			if let Ok(pos) = transactions.binary_search_by_key(&id, |transaction| transaction.id) {
				transactions.remove(pos);
			}
		});
	}

	pub fn data<D: Serialize + Send + 'static>(&self, data: D) {
		self.emit(TransactionMessage::Data(self.id, json!(data)));
	}

	pub fn status<S: Into<String>>(&self, status: S) {
		self.emit(TransactionMessage::Status(self.id, status.into()))
	}

	pub fn progress(&self, progress: f64) {
		if self.aborted() {
			dprintln!("Tried to progress an aborted transaction!");
		} else {
			self.emit(TransactionMessage::Progress(self.id, progress_as_int(progress)));
		}
	}

	pub fn progress_incr(&self, progress: f64) {
		if self.aborted() {
			dprintln!("Tried to progress an aborted transaction!");
		} else {
			self.emit(TransactionMessage::IncrProgress(self.id, progress_as_int(progress)));
		}
	}

	pub fn progress_reset(&self) {
		if self.aborted() {
			dprintln!("Tried to reset the progress of an aborted transaction!");
		} else {
			self.emit(TransactionMessage::ResetProgress(self.id));
		}
	}

	pub fn error<S: Into<String>, D: Serialize + Send + 'static>(&self, msg: S, data: D) {
		let mut state = self.state.lock();
		if state.terminal() {
			return;
		}
		*state = JobState::Failed;
		self.remove();
		self.emit(TransactionMessage::Error(self.id, msg.into(), json!(data)));
	}

	pub fn finished<D: Serialize + Send + 'static>(&self, data: D) {
		let mut state = self.state.lock();
		if state.terminal() || *state == JobState::Cancelling {
			return;
		}
		*state = JobState::Finished;
		self.remove();
		self.emit(TransactionMessage::Finished(self.id, json!(data)));
	}

	pub fn cancel(&self) -> JobState {
		let mut state = self.state.lock();
		match *state {
			JobState::Running if !self.cooperative => {
				*state = JobState::Cancelled;
				self.remove();
				self.emit(TransactionMessage::Cancelled(self.id));
			}
			JobState::Running | JobState::Preparing | JobState::Packing => {
				*state = JobState::Cancelling;
				self.emit(TransactionMessage::State(self.id, *state));
			}
			_ => {}
		}
		*state
	}

	pub fn cancelled(&self) {
		let mut state = self.state.lock();
		if *state == JobState::Cancelling {
			*state = JobState::Cancelled;
			self.remove();
			self.emit(TransactionMessage::Cancelled(self.id));
		}
	}

	pub fn begin_packing(&self) -> bool {
		let mut state = self.state.lock();
		if *state != JobState::Preparing {
			return false;
		}
		*state = JobState::Packing;
		self.emit(TransactionMessage::State(self.id, *state));
		true
	}

	/// The same lock arbitrates cancellation and the irreversible Steam submission.
	pub fn begin_submission(&self) -> bool {
		let mut state = self.state.lock();
		if !matches!(*state, JobState::Preparing | JobState::Packing | JobState::Unknown) {
			return false;
		}
		*state = JobState::Submitting;
		self.emit(TransactionMessage::State(self.id, *state));
		true
	}

	pub fn begin_commit(&self) -> bool {
		let mut state = self.state.lock();
		if state.terminal() || *state == JobState::Cancelling { return false; }
		*state = JobState::Committing;
		self.emit(TransactionMessage::State(self.id, *state));
		true
	}

	pub fn uncertain(&self) {
		let mut state = self.state.lock();
		if matches!(*state, JobState::Submitting | JobState::Preparing | JobState::Packing) {
			*state = JobState::Unknown;
			self.emit(TransactionMessage::State(self.id, *state));
		}
	}

	pub fn aborted(&self) -> bool {
		let state = *self.state.lock();
		state.terminal() || state == JobState::Cancelling
	}
}
impl Drop for TransactionInner {
	fn drop(&mut self) {
		if *self.state.get_mut() == JobState::Unknown {
			self.remove();
		} else if *self.state.get_mut() == JobState::Cancelling {
			self.cancelled();
		} else if !self.state.get_mut().terminal() {
			self.error("ERR_UNKNOWN", turbonone!());

			#[cfg(debug_assertions)]
			println!("{:#?}", backtrace::Backtrace::new());
		}
	}
}
impl serde::Serialize for TransactionInner {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		serializer.serialize_u64(self.id as u64)
	}
}

pub fn init() {
	lazy_static::initialize(&TRANSACTIONS);
	snapshots::init();
}

pub fn new() -> Transaction {
	new_with_state(JobState::Running, false)
}

pub fn new_publish() -> Transaction {
	let transaction = new_with_state(JobState::Preparing, true);
	transaction.emit(TransactionMessage::State(transaction.id, JobState::Preparing));
	transaction
}

pub fn new_extraction() -> Transaction {
	new_with_state(JobState::Running, true)
}

fn new_with_state(state: JobState, cooperative: bool) -> Transaction {
	let mut transactions = TRANSACTIONS.write();
	let transaction = Arc::new(TransactionInner {
		id: TRANSACTIONS.id.fetch_add(1, Ordering::SeqCst),
		state: Mutex::new(state),
		cooperative,
	});

	transactions.push(TransactionRef {
		id: transaction.id,
		ptr: Arc::downgrade(&transaction),
	});
	transactions.reserve(1);

	snapshots::insert(transaction.id, state);
	transaction
}

#[macro_export]
macro_rules! transaction {
	() => {
		$crate::transactions::new()
	};
}

#[tauri::command]
pub fn cancel_transaction(id: u32) -> Result<JobState, String> {
	TRANSACTIONS
		.find(id)
		.map(|transaction| transaction.cancel())
		.ok_or_else(|| "ERR_TRANSACTION_NOT_FOUND".to_owned())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::Barrier;

	#[test]
	fn extraction_cancellation_is_acknowledged_by_the_worker() {
		let job = new_extraction();
		assert_eq!(job.cancel(), JobState::Cancelling);
		assert!(!job.begin_commit());
		assert_eq!(*job.state.lock(), JobState::Cancelling);
		job.cancelled();
		assert_eq!(*job.state.lock(), JobState::Cancelled);
		let job = new_extraction();
		assert!(job.begin_commit());
		assert_eq!(job.cancel(), JobState::Committing);
		job.finished(());
	}

	#[test]
	fn snapshots_replay_spilled_data_before_the_terminal_result() {
		let job = new();
		for index in 0..400 { job.data(serde_json::json!([index, "entry"])); }
		job.finished("done");
		let mut snapshot = transaction_snapshot(job.id).unwrap().unwrap();
		let mut received = Vec::new();
		loop {
			received.extend(snapshot.data.iter().map(|(_, value)| value[0].as_u64().unwrap()));
			if !snapshot.data_more { break; }
			let after = snapshot.data.back().unwrap().0;
			snapshot = transaction_data(job.id, after).unwrap().unwrap();
		}
		assert_eq!(received, (0..400).collect::<Vec<_>>());
		assert_eq!(snapshot.state, JobState::Finished);
		assert_eq!(snapshot.result, Some(json!("done")));
		acknowledge_transaction(job.id).unwrap();
	}

	#[test]
	fn cancellation_prevents_submission_and_waits_for_cleanup() {
		for packing in [false, true] {
			let transaction = new_publish();
			if packing {
				assert!(transaction.begin_packing());
			}
			assert_eq!(cancel_transaction(transaction.id).unwrap(), JobState::Cancelling);
			assert!(transaction.aborted());
			assert!(!transaction.begin_packing());
			assert!(!transaction.begin_submission());
			transaction.finished(());
			assert_eq!(*transaction.state.lock(), JobState::Cancelling);
			assert_eq!(transaction.cancel(), JobState::Cancelling);
			transaction.cancelled();
			transaction.cancelled();
			transaction.error("late error", ());
			assert_eq!(*transaction.state.lock(), JobState::Cancelled);
		}
	}

	#[test]
	fn submission_rejects_cancellation_and_preserves_the_steam_result() {
		for failure in [false, true] {
			let transaction = new_publish();
			assert!(transaction.begin_submission());
			assert_eq!(cancel_transaction(transaction.id).unwrap(), JobState::Submitting);
			assert!(!transaction.aborted());
			assert!(!transaction.begin_submission());
			transaction.cancelled();
			assert_eq!(*transaction.state.lock(), JobState::Submitting);
			if failure {
				transaction.error("Steam failure", ());
			} else {
				transaction.finished(());
			}
			let expected = if failure { JobState::Failed } else { JobState::Finished };
			assert_eq!(transaction.cancel(), expected);
			transaction.finished(());
			transaction.error("late error", ());
			assert_eq!(*transaction.state.lock(), expected);
		}
	}

	#[test]
	fn cancellation_and_submission_have_exactly_one_winner() {
		for _ in 0..100 {
			let transaction = new_publish();
			assert!(transaction.begin_packing());
			let barrier = Arc::new(Barrier::new(2));
			let submit = transaction.clone();
			let start = barrier.clone();
			let thread = std::thread::spawn(move || {
				start.wait();
				submit.begin_submission()
			});
			barrier.wait();
			let cancellation = transaction.cancel();
			let submitted = thread.join().unwrap();
			assert_eq!(submitted, cancellation == JobState::Submitting);
			assert_eq!(!submitted, cancellation == JobState::Cancelling);
			if submitted {
				transaction.finished(());
			} else {
				transaction.cancelled();
			}
		}
	}

	#[test]
	fn cleanup_errors_are_reported_and_unknown_jobs_are_rejected() {
		let transaction = new_publish();
		transaction.cancel();
		transaction.error("cleanup failed", ());
		transaction.cancelled();
		assert_eq!(*transaction.state.lock(), JobState::Failed);
		assert_eq!(cancel_transaction(u32::MAX).unwrap_err(), "ERR_TRANSACTION_NOT_FOUND");
	}
}
