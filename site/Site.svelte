<script>
	import { onMount } from 'svelte';
	import { _ } from 'svelte-i18n';
	import { ArrowDown, ArrowUpRight, Download, CodeXml } from '@lucide/svelte';
	import logo from '../public/img/logo.svg';
	import windows from '../public/img/windows.svg';
	import apple from '../public/img/apple.svg';
	import linux from '../public/img/linux.svg';
	import workshop from '../public/screenshots/My workshop.webp';
	import publishing from '../public/screenshots/Publish New.webp';
	import size from '../public/screenshots/Addon Size Analyzer.webp';
	import editing from '../public/screenshots/Edit.webp';
	import addonSettings from '../public/screenshots/Addon Settings.webp';
	import extraction from '../public/screenshots/Addon Extract.webp';
	import { detectPlatform, getLatestRelease, repositoryUrl, releasesUrl } from './downloads.js';

	const screenshots = [
		{ id: 'workshop', image: workshop },
		{ id: 'publishing', image: publishing },
		{ id: 'size', image: size }
	];
	const features = [
		{ id: 'publish', image: editing, screen: 'edit' },
		{ id: 'manage', image: addonSettings, screen: 'addon_settings' },
		{ id: 'clean', image: extraction, screen: 'extract' }
	];
	const platforms = [
		{ id: 'windows', icon: windows, packages: ['windows', 'windows_portable'] },
		{ id: 'macos', icon: apple, packages: ['macos'] },
		{ id: 'linux', icon: linux, packages: ['linux', 'deb', 'rpm'] }
	];
	let screenshot = screenshots[0];
	let detectedPlatform = null;
	let release = null;
	let loading = true;
	let failed = false;
	$: recommendedDownload = release?.downloads[detectedPlatform];

	onMount(() => {
		detectedPlatform = detectPlatform(navigator);
		const controller = new AbortController();
		let active = true;
		const timeout = setTimeout(() => controller.abort(), 10000);
		getLatestRelease(controller.signal).then(result => {
			if (active) release = result;
		}).catch(error => {
			if (!active) return;
			console.error('Could not load release downloads:', error);
			failed = true;
		}).finally(() => {
			clearTimeout(timeout);
			if (active) loading = false;
		});
		return () => {
			active = false;
			clearTimeout(timeout);
			controller.abort();
		};
	});
</script>

<a class="skip-link" href="#main">{$_('website.skip')}</a>

<header id="top" class="site-header">
	<a class="brand" href="#top" aria-label="nwmpublisher">
		<img src={logo} alt="" width="36" height="36"/>
		<span>nwmpublisher</span>
	</a>
	<nav aria-label={$_('website.navigation')}>
		<a href="#features">{$_('website.features')}</a>
		<a href="#downloads">{$_('website.downloads')}</a>
		<a class="source-link" href={repositoryUrl}><CodeXml size={18}/> <span>{$_('website.source')}</span></a>
	</nav>
</header>

<main id="main">
	<section class="hero" aria-labelledby="hero-title">
		<div class="hero-copy">
			<p class="eyebrow"><a href="https://github.com/WilliamVenner/gmpublisher">{$_('website.eyebrow')}</a></p>
			<h1 id="hero-title">{$_('website.heading')}</h1>
			<p class="intro">{$_('website.intro')}</p>
			<div class="hero-actions">
				{#if recommendedDownload}
					<a class="button primary" href={recommendedDownload}><Download size={19}/>{$_(`website.download_${detectedPlatform}`)}</a>
				{:else}
					<a class="button primary" href="#downloads"><Download size={19}/>{$_('website.download')}</a>
				{/if}
				<a class="text-link" href="#downloads">{$_('website.all_platforms')}<ArrowDown size={16}/></a>
			</div>
			<p class="platform-note">{$_('website.platforms')}</p>
		</div>
		<figure class="app-preview">
			<div class="preview-tabs" role="group" aria-label={$_('website.screenshots')}>
				{#each screenshots as item}
					<button type="button" aria-pressed={screenshot.id === item.id} on:click={() => screenshot = item}>
						{$_(`website.screen_${item.id}`)}
					</button>
				{/each}
			</div>
			<a class="screenshot-link" href={screenshot.image} target="_blank" rel="noreferrer" aria-label={$_('website.enlarge', { values: { name: $_(`website.screen_${screenshot.id}`) } })}>
				<img src={screenshot.image} alt={$_(`website.alt_${screenshot.id}`)} width="2560" height="1392" fetchpriority="high"/>
			</a>
			<figcaption>{$_(`website.caption_${screenshot.id}`)}<ArrowUpRight size={15}/></figcaption>
		</figure>
	</section>

	<section id="features" class="features" aria-labelledby="features-title">
		<div class="section-heading">
			<h2 id="features-title">{$_('website.features_heading')}</h2>
		</div>
		<div class="feature-list">
			{#each features as feature}
				<article>
					<div class="feature-copy">
						<h3>{$_(`website.feature_${feature.id}_title`)}</h3>
						<p>{$_(`website.feature_${feature.id}_body`)}</p>
					</div>
					<figure class="feature-preview">
						<a class="screenshot-link" href={feature.image} target="_blank" rel="noreferrer" aria-label={$_('website.enlarge', { values: { name: $_(`website.screen_${feature.screen}`) } })}>
							<img src={feature.image} alt={$_(`website.alt_${feature.screen}`)} width="2560" height="1392" loading="lazy"/>
						</a>
						<figcaption>{$_(`website.caption_${feature.screen}`)}<ArrowUpRight size={15}/></figcaption>
					</figure>
				</article>
			{/each}
		</div>
	</section>

	<section id="downloads" class="downloads" aria-labelledby="downloads-title">
		<div class="section-heading download-heading">
			<div>
				<h2 id="downloads-title">{$_('website.download_heading')}</h2>
				<p>{$_('website.download_intro')}</p>
			</div>
			{#if release}<a class="release-version" href={releasesUrl}>{$_('website.release', { values: { version: release.version } })}<ArrowUpRight size={15}/></a>{/if}
		</div>
		<p class="release-status" role="status">
			{#if loading}{$_('website.loading')}{:else if failed}{$_('website.load_error')}{/if}
		</p>
		<div class="download-grid">
			{#each platforms as platform}
				<article class:detected={detectedPlatform === platform.id}>
					<div class="platform-heading">
						<img src={platform.icon} alt="" width="25" height="25"/>
						{#if detectedPlatform === platform.id}<span class="detected-label">{$_('website.detected')}</span>{/if}
					</div>
					<h3>{$_(`website.platform_${platform.id}`)}</h3>
					<p class="architecture">{$_(`website.arch_${platform.id}`)}</p>
					<div class="package-links">
						{#each platform.packages as pkg}
							{#if release?.downloads[pkg]}
								<a class="button package" href={release.downloads[pkg]}><Download size={16}/>{$_(`website.package_${pkg}`)}</a>
							{:else}
								<span class="button unavailable">{$_(`website.package_${pkg}`)}<span>{$_(loading ? 'website.pending' : 'website.unavailable')}</span></span>
							{/if}
						{/each}
					</div>
					<p class="install-note">{$_(`website.install_${platform.id}`)}</p>
					{#if platform.id === 'windows'}
						<p class="install-note">{$_('website.install_windows_portable')}</p>
					{/if}
				</article>
			{/each}
		</div>
		<div class="download-footer">
			<p>{$_('website.steam_note')}</p>
			<a class="text-link" href={releasesUrl}>{$_('website.releases_link')}<ArrowUpRight size={16}/></a>
		</div>
	</section>
</main>

<footer class="site-footer">
	<div class="footer-brand"><img src={logo} alt="" width="25" height="25"/><span>nwmpublisher</span></div>
	<p><a href="https://github.com/DeisDev">{$_('website.maintained')}</a><br/><a href="https://github.com/WilliamVenner/gmpublisher">{$_('website.credits')}</a></p>
	<a class="text-link" href={repositoryUrl}><CodeXml size={17}/>{$_('website.source')}</a>
</footer>
