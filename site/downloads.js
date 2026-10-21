export const repositoryUrl = 'https://github.com/DeisDev/nwmpublisher';
export const releasesUrl = `${repositoryUrl}/releases/latest`;
export const nightlyUrl = `${repositoryUrl}/releases/tag/nightly`;
const releasesApi = 'https://api.github.com/repos/DeisDev/nwmpublisher/releases';
const nightlyTitle = /^nwmpublisher-nightly (\d+\.\d+\.\d+\.\d+)$/;

export function detectPlatform(browser) {
	const userAgent = browser.userAgent || '';
	const platform = browser.userAgentData?.platform || browser.platform || userAgent;
	if (browser.userAgentData?.mobile || /Android|iPhone|iPad|iPod|CrOS/i.test(userAgent)
		|| (/Mac/i.test(platform) && browser.maxTouchPoints > 1)) return null;
	if (/Windows|^Win32$|^Win64$/i.test(platform)) return 'windows';
	if (/Mac/i.test(platform)) return 'macos';
	if (/Linux/i.test(platform)) return 'linux';
	return null;
}

async function fetchRelease(path, signal) {
	const response = await fetch(`${releasesApi}/${path}`, {
		headers: { Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2026-03-10' },
		signal
	});
	if (!response.ok) throw new Error(`GitHub release request failed: HTTP ${response.status}`);
	return response.json();
}

function findDownloads(assets) {
	const downloads = {};
	const packages = {
		windows: /_x64[^/]*\.msi$/i,
		windows_portable: /_x64_portable\.zip$/i,
		macos: /_universal\.dmg$/i,
		linux: /_amd64\.AppImage$/i,
		deb: /_amd64\.deb$/i,
		rpm: /\.x86_64\.rpm$/i
	};
	for (const [platform, pattern] of Object.entries(packages)) {
		const asset = assets.find(asset => typeof asset.name === 'string' && pattern.test(asset.name));
		if (!asset) continue;
		if (typeof asset.browser_download_url !== 'string'
			|| !asset.browser_download_url.startsWith(`${repositoryUrl}/releases/download/`)) {
			throw new Error(`GitHub returned an invalid download URL for ${platform}`);
		}
		downloads[platform] = asset.browser_download_url;
	}
	return downloads;
}

export async function getLatestRelease(signal) {
	const release = await fetchRelease('latest', signal);
	if (typeof release.tag_name !== 'string' || !Array.isArray(release.assets) || release.draft || release.prerelease) {
		throw new Error('GitHub returned an invalid stable release');
	}
	return { version: release.tag_name, downloads: findDownloads(release.assets) };
}

export async function getNightlyRelease(signal) {
	const release = await fetchRelease('tags/nightly', signal);
	const version = typeof release.name === 'string' ? release.name.match(nightlyTitle)?.[1] : null;
	if (release.tag_name !== 'nightly' || !version || !Array.isArray(release.assets) || release.draft || !release.prerelease) {
		throw new Error('GitHub returned an invalid nightly release');
	}
	return { version, downloads: findDownloads(release.assets) };
}
