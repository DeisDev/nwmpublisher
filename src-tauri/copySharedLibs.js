import fs from "fs";
import { execFileSync } from "node:child_process";

let steamApiDir;
let steamApiFile;
if (process.platform === "win32") {
	steamApiFile = 'steam_api64.dll';
	steamApiDir = 'win64';
} else if (process.platform === "darwin") {
	steamApiFile = 'libsteam_api.dylib';
	steamApiDir = 'osx';
} else {
	steamApiFile = 'libsteam_api.so';
	steamApiDir = 'linux64';
}

fs.mkdirSync('src-tauri/target/debug', { recursive: true });
fs.mkdirSync('src-tauri/target/release', { recursive: true });

const path = `src-tauri/lib/steam_api/redistributable_bin/${steamApiDir}/${steamApiFile}`;
for (const profile of ['debug', 'release']) {
	const destination = `src-tauri/target/${profile}/${steamApiFile}`;
	fs.copyFileSync(path, destination);
	if (process.platform === 'darwin') {
		// Sign nested code before Tauri seals the app; the SDK uses @loader_path.
		const identity = process.env.APPLE_SIGNING_IDENTITY || '-';
		const args = ['--force', '--sign', identity];
		if (identity !== '-') args.push('--timestamp');
		execFileSync('codesign', [...args, destination], { stdio: 'inherit' });
	}
}
