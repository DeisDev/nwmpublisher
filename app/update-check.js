const LATEST_RELEASE = 'https://api.github.com/repos/DeisDev/nwmpublisher/releases/latest';
const RETRY_DELAY_MS = 30000;
const STABLE_VERSION = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

function versionParts(version) {
	if (typeof version !== 'string') return null;
	const match = version.match(STABLE_VERSION);
	if (!match) return null;
	const parts = match.slice(1, 4).map(Number);
	return parts.every(Number.isSafeInteger) ? parts : null;
}

export function isNewerVersion(current, candidate) {
	const currentParts = versionParts(current);
	const candidateParts = versionParts(candidate);
	if (!currentParts || !candidateParts) return false;

	for (let i = 0; i < 3; i++) {
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

async function fetchUpdateVersion(current, fetchRelease) {
	const response = await fetchRelease(LATEST_RELEASE);
	if (!response.ok) throw new UpdateCheckError(response.status);
	const release = await response.json();
	if (!release || release.draft || release.prerelease) return null;
	return isNewerVersion(current, release.tag_name) ? release.tag_name : null;
}

// GitHub asks clients not to retry rate-limited requests (403/429) before the limit resets.
export async function getUpdateVersion(current, { retry = true, fetchRelease = globalThis.fetch } = {}) {
	try {
		return await fetchUpdateVersion(current, fetchRelease);
	} catch (error) {
		const rateLimited = error instanceof UpdateCheckError && (error.status === 403 || error.status === 429);
		if (!retry || rateLimited) throw error;
		await new Promise(resolve => setTimeout(resolve, RETRY_DELAY_MS));
		return fetchUpdateVersion(current, fetchRelease);
	}
}
