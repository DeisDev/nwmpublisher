window.__NWMPUBLISHER__ = async appDataCallback => {
	{
		const AppDataPtr = {};
		window.AppData = new Proxy(AppDataPtr, {
			get: function(_, key) { return _._[key]; }
		});

		function updateAppData(newAppData) {
			console.log('UpdateAppData');
			console.log(newAppData);
			console.log(newAppData.settings);

			window.AppSettings = newAppData.settings;

			delete newAppData.settings;
			AppDataPtr._ = Object.freeze(newAppData);

			if (appDataCallback) appDataCallback();
		}

		updateAppData(JSON.parse('{$_APP_DATA_$}'));
		await __TAURI__.event.listen('UpdateAppData', ({ payload }) => updateAppData(payload));

		// Fetch current settings before mounting components that read them on startup.
		updateAppData(await __TAURI__.core.invoke('reloaded'));
	}
};

window.__WS_DEAD__ = JSON.parse('{$_WS_DEAD_$}');
window.__WS_DEAD__.dead = true;
delete window.__WS_DEAD__.id;
delete window.__WS_DEAD__.title;
delete window.__WS_DEAD__.searchTitle;
delete window.__WS_DEAD__.localFile;

window.PATH_SEPARATOR = {$_PATH_SEPARATOR_$};

window.DEFAULT_IGNORE_GLOBS = JSON.parse('{$_DEFAULT_IGNORE_GLOBS_$}');

let resizeTimeout;
function resized() {
	window.__TAURI__.core.invoke("window_resized", {
		width: window.innerWidth,
		height: window.innerHeight
	}).catch(error => window.dispatchEvent(new CustomEvent('settings-save-error', { detail: error })));
}
window.addEventListener('resize', e => {
	clearTimeout(resizeTimeout);
	resizeTimeout = setTimeout(resized, 400, e);
});
