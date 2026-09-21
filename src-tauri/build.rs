fn main() {
	if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
		// Support both development binaries and the private library directory in Linux packages.
		println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN:$ORIGIN/../lib/nwmpublisher");
	}

	tauri_build::build()
}
