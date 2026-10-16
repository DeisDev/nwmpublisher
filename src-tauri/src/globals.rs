pub const GMOD_APP_ID: steamworks::AppId = steamworks::AppId(4000);

pub static STEAMWORKS: std::sync::LazyLock<crate::steam::Steam> = std::sync::LazyLock::new(crate::steam::Steam::init);
pub static GAME_ADDONS: std::sync::LazyLock<crate::game_addons::GameAddons> = std::sync::LazyLock::new(crate::game_addons::GameAddons::init);
pub static ADDON_SIZE_ANALYZER: std::sync::LazyLock<crate::addon_size_analyzer::AddonSizeAnalyzer> =
	std::sync::LazyLock::new(crate::addon_size_analyzer::AddonSizeAnalyzer::init);
pub static APP_DATA: std::sync::LazyLock<crate::appdata::AppData> = std::sync::LazyLock::new(crate::appdata::AppData::init);
pub static WEBVIEW: crate::webview::WrappedWebview = crate::webview::WrappedWebview::pending();
pub static SEARCH: std::sync::LazyLock<crate::search::Search> = std::sync::LazyLock::new(crate::search::Search::init);

#[macro_export]
macro_rules! steam {
	() => {
		&$crate::STEAMWORKS
	};
}
#[macro_export]
macro_rules! downloads {
	() => {
		&$crate::steam::DOWNLOADS
	};
}

#[macro_export]
macro_rules! game_addons {
	() => {
		&$crate::GAME_ADDONS
	};
}

#[macro_export]
macro_rules! app_data {
	() => {
		&$crate::APP_DATA
	};
}

#[macro_export]
macro_rules! search {
	() => {
		&$crate::SEARCH
	};
}

#[macro_export]
macro_rules! webview {
	() => {
		&$crate::WEBVIEW
	};
}
#[macro_export]
macro_rules! webview_emit {
	( $event:expr, $data:expr ) => {
		$crate::webview!().emit($event, Some($data))
	};

	( $event:expr ) => {
		$crate::webview!().emit($event, turbonone!())
	};
}

pub(super) fn init_globals() {
	println!("Initializing Steamworks...");
	std::sync::LazyLock::force(&STEAMWORKS);

	println!("Initializing AppData...");
	std::sync::LazyLock::force(&APP_DATA);

	println!("Initializing Transactions...");
	crate::transactions::init();

	rayon::spawn(|| {
		println!("Initializing Game Addons...");
		std::sync::LazyLock::force(&GAME_ADDONS);
		GAME_ADDONS.discover_addons();
	});

	rayon::spawn(|| {
		std::sync::LazyLock::force(&crate::gma::whitelist::ADDON_WHITELIST);
	});
}
