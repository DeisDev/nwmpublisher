fn main() {
	if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
		// Support both development binaries and the private library directory in Linux packages.
		println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN:$ORIGIN/../lib/nwmpublisher");
	}

	set_release_channel();
	tauri_build::build()
}

fn set_release_channel() {
	println!("cargo:rerun-if-env-changed=NWMPUBLISHER_NIGHTLY");
	println!("cargo:rerun-if-env-changed=NWMPUBLISHER_COMMIT");
	let version = std::env::var("CARGO_PKG_VERSION").unwrap();
	// The build workflows pass an empty value for stable builds.
	let nightly = std::env::var("NWMPUBLISHER_NIGHTLY").unwrap_or_default();
	if nightly.is_empty() {
		println!("cargo:rustc-env=NWMPUBLISHER_NAME=nwmpublisher");
		println!("cargo:rustc-env=NWMPUBLISHER_VERSION={version}");
		return;
	}

	let build = nightly.strip_prefix(&format!("{version}.")).unwrap_or_default();
	if build.is_empty() || !build.bytes().all(|byte| byte.is_ascii_digit()) || build.parse::<u16>().is_err() {
		panic!("NWMPUBLISHER_NIGHTLY must be {version}.<build> with a build number up to 65535, not {nightly:?}");
	}
	let commit = std::env::var("NWMPUBLISHER_COMMIT").unwrap_or_default();
	if !(7..=40).contains(&commit.len()) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
		panic!("NWMPUBLISHER_COMMIT must be the nightly build's Git commit hash, not {commit:?}");
	}
	println!("cargo:rustc-env=NWMPUBLISHER_NAME=nwmpublisher-nightly");
	println!("cargo:rustc-env=NWMPUBLISHER_VERSION={nightly} ({})", &commit[..7]);
	println!("cargo:rustc-env=NWMPUBLISHER_NIGHTLY_VERSION={nightly}");
}
