const RELEASES = 'https://api.github.com/repos/DeisDev/nwmpublisher/releases';
const RELEASE_PAGES = 'https://github.com/DeisDev/nwmpublisher/releases/tag/';
const RETRY_DELAY_MS = 30000;
const STABLE_VERSION = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;
const NIGHTLY_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const NIGHTLY_TITLE = /^nwmpublisher-nightly (\S+)$/;

function versionParts(version, pattern) {
	if (typeof version !== 'string') return null;
	const match = version.match(pattern);
	if (!match) return null;
	const parts = match.slice(1).map(Number);
	return parts.every(Number.isSafeInteger) ? parts : null;
}

export function isNewerVersion(current, candidate, pattern = STABLE_VERSION) {
	const currentParts = versionParts(current, pattern);
	const candidateParts = versionParts(candidate, pattern);
	if (!currentParts || !candidateParts) return false;

	for (let i = 0; i < currentParts.length; i++) {
		if (currentParts[i] !== candidateParts[i]) {
			return candidateParts[i] > currentParts[i];
		}
	}
	return false;
}

class UpdateCheckError extends Error {
	constructor(status) {
		super(`GitHub returned HTTP ${status}`);
		this.status = status;
	}
}

async function fetchUpdate(current, nightly, fetchRelease) {
	const response = await fetchRelease(`${RELEASES}/${nightly ? 'tags/nightly' : 'latest'}`);
	if (!response.ok) throw new UpdateCheckError(response.status);
	const release = await response.json();
	if (!release || release.draft) return null;

	if (nightly) {
		if (release.tag_name !== 'nightly' || !release.prerelease || typeof release.name !== 'string') return null;
		const version = release.name.match(NIGHTLY_TITLE)?.[1];
		return isNewerVersion(current, version, NIGHTLY_VERSION) ? { version, url: RELEASE_PAGES + 'nightly' } : null;
	}
	if (release.prerelease) return null;
	return isNewerVersion(current, release.tag_name) ? { version: release.tag_name, url: RELEASE_PAGES + encodeURIComponent(release.tag_name) } : null;
}

// GitHub asks clients not to retry rate-limited requests (403/429) before the limit resets.
export async function getUpdate(current, { nightly = false, retry = true, fetchRelease = globalThis.fetch } = {}) {
	try {
		return await fetchUpdate(current, nightly, fetchRelease);
	} catch (error) {
		const rateLimited = error instanceof UpdateCheckError && (error.status === 403 || error.status === 429);
		if (!retry || rateLimited) throw error;
		await new Promise(resolve => setTimeout(resolve, RETRY_DELAY_MS));
		return fetchUpdate(current, nightly, fetchRelease);
	}
}
