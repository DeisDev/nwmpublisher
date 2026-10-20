window.DEBUG_MODE = {$_DEBUG_MODE_$};

console.log('Listening for errors');

(function() {

	const eprintln = window.console.error;

	function report(message, data) {
		const invoke = __TAURI__?.core?.invoke;

		if (invoke) invoke(message, data);
		else eprintln('Failed to report log to Tauri backend!');
	}

	function stringify(...data) {
		return data.map(value => {
			if (typeof value === 'string') return value;
			if (value instanceof Error) return value.stack || String(value);
			return JSON.stringify(value, null, 4);
		}).join('\t');
	}

	let _error = window.console.error;
	let _warn = window.console.warn;
	let _log = window.console.log;

	window.console.warn = (...data) => {
		report('warn', { message: stringify(...data) });
		_warn(...data);
	};

	if (DEBUG_MODE) {
		window.console.log = (...data) => {
			report('info', { message: stringify(...data) });
			_log(...data);
		};
	}

	{
		_error = window.console.log;
		window.console.error = (...data) => {
			report('error', { message: stringify(...data) });
			_error(...data);
		};
	}

	function describe(value) {
		if (value instanceof Error) return { message: value.message, stack: value.stack ?? '' };
		return { message: typeof value === 'string' ? value : String(stringify(value)), stack: '' };
	}

	window.addEventListener('error', e => {
		const { message, stack } = describe(e.error ?? e.message);
		report('js_error', {
			message,
			stack: stack || `${e.filename ?? ''}:${e.lineno ?? 0}:${e.colno ?? 0}`
		});
	});

	window.addEventListener('unhandledrejection', e => {
		const { message, stack } = describe(e.reason);
		report('js_error', { message: 'Unhandled promise rejection: ' + message, stack });
	});

	window.console.report = (msgType, ...data) => {
		switch (msgType) {
			case 'error': {
				report('error', { message: stringify(...data) });
				_error(...data);
				break;
			}

			case 'warn': {
				report('warn', { message: stringify(...data) });
				_warn(...data);
				break;
			}

			default: {
				report('info', { message: stringify(...data) });
				_log(...data);
			}
		}
	};

})();
