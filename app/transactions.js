import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { writable } from 'svelte/store';

let transactions = {};

const tasks = writable([]);
const tasksNum = writable(0);
const taskHeight = 49;
const tasks1080p = 4;
const ratio = (taskHeight * tasks1080p) / 1027;
const tasksMax = writable(4);
function resize() {
	tasksMax.set(Math.max(
		Math.round(
			(window.outerHeight * ratio) / taskHeight
		),
		2
	));
}
window.addEventListener('resize', resize);
resize();

export function taskMessage(msg) {
	tasks.update(tasks => { tasks.push([null, msg, null]); return tasks });
}

const completed = new Set();
function acknowledge(id) {
	completed.add(id);
	if (completed.size > 1024) completed.delete(completed.values().next().value);
	invoke('acknowledge_transaction', { id }).catch(error => console.error(error));
}

class Transaction {
	constructor(id, TASK_statusTextFn, cancellable = true) {
		if (id === null || id == undefined) return;

		if (transactions[id]) return transactions[id];

		this.id = id;
		this.callbacks = [];
		this.progress = 0;
		this.finished = false;
		this.cancelled = false;
		this.cancellable = cancellable;
		this.state = 'running';
		this.cancelPending = false;
		this.unconsumedEvents = [];

		transactions[id] = this;

		if (TASK_statusTextFn)
			tasks.update(tasks => { tasks.push([this, TASK_statusTextFn, null]); return tasks });

		this.revision = -1;
		this.dataRevision = 0;
		if (id !== -1) this.refresh();
	}

	async refresh() {
		try {
			const snapshot = await invoke('transaction_snapshot', { id: this.id });
			this.transportError = null;
			if (snapshot) this.applySnapshot(snapshot);
		} catch (error) {
			this.transportError = String(error);
			this.emit({ transportError: this.transportError });
		}
	}

	applySnapshot(snapshot) {
		if (snapshot.revision <= this.revision || this.finished || this.error || this.cancelled) return;
		if (!this.pendingSnapshot || snapshot.revision >= this.pendingSnapshot.revision) this.pendingSnapshot = snapshot;
		if (!this.draining) this.drainSnapshots();
	}

	async drainSnapshots() {
		this.draining = true;
		try {
			while (this.pendingSnapshot) {
				let snapshot = this.pendingSnapshot;
				this.pendingSnapshot = null;
				if (snapshot.revision <= this.revision) continue;
				this.context = snapshot.context;
				do {
					for (const [sequence, data] of snapshot.data) {
						if (sequence > this.dataRevision) { this.dataRevision = sequence; this.setData(data); }
					}
					if (!snapshot.dataMore) break;
					snapshot = await invoke('transaction_data', { id: this.id, after: this.dataRevision });
					if (!snapshot) throw new Error('ERR_JOB_DATA_DELIVERY');
				} while (true);
				this.transportError = null;
				this.revision = snapshot.revision;
				this.warnings = snapshot.warnings;
				if (this.warnings?.length) this.emit({ warnings: this.warnings });
				if (snapshot.status !== null && snapshot.status !== this.status) this.setStatus(snapshot.status);
				this.setProgress(snapshot.progress);
				this.setState(snapshot.state);
				if (snapshot.state === 'finished') this.setFinished(snapshot.result);
				else if (snapshot.state === 'failed') this.setError(...snapshot.error);
				else if (snapshot.state === 'cancelled') this.setCancelled();
				else if (snapshot.data.length) await invoke('transaction_data', { id: this.id, after: this.dataRevision });
			}
		} catch (error) {
			this.transportError = String(error);
			this.emit({ transportError: this.transportError });
		} finally { this.draining = false; }
	}

	static get(id) {
		return transactions[id];
	}

	listen(callback) {
		this.callbacks.push(callback);

		if (this.callbacks.length === 1) {
			for (const event of this.unconsumedEvents.splice(0)) callback(event);
		}

		return this;
	}

	emit(event) {
		if (this.callbacks.length === 0) {
			const key = event.stream ? null : Object.keys(event)[0];
			if (key) this.unconsumedEvents = this.unconsumedEvents.filter(previous => !(key in previous));
			this.unconsumedEvents.push(event);
			if (this.unconsumedEvents.length > 256) this.unconsumedEvents.shift();
		} else {
			for (let i = 0; i < this.callbacks.length; i++) {
				this.callbacks[i](event);
			}
		}
		return this;
	}

	async cancel() {
		if (this.cancelled || this.finished || this.error || !this.cancellable || this.cancelPending) return;
		this.cancelPending = true;
		this.emit({ cancelPending: true });
		try {
			const state = await invoke('cancel_transaction', { id: this.id });
			if (state === 'cancelled') this.setCancelled();
			else this.setState(state);
		} catch (error) {
			if (!this.cancelled && !this.finished && !this.error) {
				this.emit({ cancelError: String(error) });
			}
		} finally {
			this.cancelPending = false;
			this.emit({ cancelPending: false });
		}
	}

	setState(state) {
		if (this.cancelled || this.finished || this.error) return this;
		const order = { running: 0, preparing: 1, packing: 2, submitting: 3, cancelling: 3, committing: 3, unknown: 4 };
		if (!(state in order) || order[state] < order[this.state]) return this;
		this.state = state;
		this.cancellable = state === 'running' || state === 'preparing' || state === 'packing';
		this.emit({ state });
		return this;
	}

	setCancelled() {
		if (this.cancelled || this.finished || this.error) return this;
		this.state = 'cancelled';
		this.cancelled = true;
		this.cancellable = false;
		this.emit({ cancelled: true });
		delete transactions[this.id];
		acknowledge(this.id);
		return this;
	}

	setFinished(data) {
		if (this.cancelled || this.finished || this.error) return this;
		this.result = data;
		this.state = 'finished';
		this.cancellable = false;
		this.finished = true;
		if (this.progress < 100) {
			this.progress = 100;
			this.emit({ progress: 100 });
		}
		this.emit({ finished: true, data });
		delete transactions[this.id];
		acknowledge(this.id);

		return this;
	}

	setError(msg, data) {
		if (this.cancelled || this.finished || this.error) return this;
		this.state = 'failed';
		this.cancellable = false;
		this.error = [msg, data];
		this.emit({ error: msg, data });
		delete transactions[this.id];
		acknowledge(this.id);

		return this;
	}

	setData(data) {
		this.emit({ stream: true, data });
		return this;
	};

	setStatus(msg) {
		this.status = msg;
		this.emit({ msg });

		return this;
	}

	setProgress(progress) {
		if (progress !== this.progressInt) {
			this.progressInt = progress;
			this.progress = progress / 100;
			this.emit({ progress: this.progress });
		}

		return this;
	}
}

function receiveSnapshot(snapshot) {
	const transaction = Transaction.get(snapshot.id);
	if (transaction) {
		if (transaction.transportError) { transaction.transportError = null; transaction.emit({ transportError: null }); }
		transaction.applySnapshot(snapshot);
	} else if (completed.has(snapshot.id)) acknowledge(snapshot.id);
	else if (snapshot.context && !completed.has(snapshot.id)) {
		window.dispatchEvent(new CustomEvent('recovered-job', { detail: snapshot }));
	}
}

let unlisten;
let recovering = false;
async function recoverTransactions() {
	if (recovering) return;
	recovering = true;
	try {
		if (!unlisten) unlisten = await listen('TransactionSnapshot', ({ payload }) => receiveSnapshot(payload));
		for (const snapshot of await invoke('transaction_snapshots')) receiveSnapshot(snapshot);
	} catch (error) {
		for (const transaction of Object.values(transactions)) {
			transaction.transportError = String(error);
			transaction.emit({ transportError: String(error) });
		}
	} finally { recovering = false; }
}
recoverTransactions();
const recoveryTimer = setInterval(recoverTransactions, 5000);
window.addEventListener('focus', recoverTransactions);
window.addEventListener('beforeunload', () => { clearInterval(recoveryTimer); unlisten?.(); });

export { Transaction, tasks, taskHeight, tasksMax, tasksNum }
