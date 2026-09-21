<script>
	import { _, locale } from 'svelte-i18n';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import { formatSize } from '../format.js';
	import Timestamp from './Timestamp.svelte';

	export let item;
	export let estimatedSize = null;
	let expanded = false;
	$: numberLocale = $locale === 'kr' ? 'ko' : ($locale ?? 'en');
	$: metrics = ['subscriptions', 'favorites', 'views', 'comments', 'upvotes', 'downvotes'];
</script>

{#if item}
	<section class="workshop-stats" aria-label={$_('workshop_details')}>
		{#if item.banned}
			<div class="banned" role="status"><ShieldAlert size="1rem"/>{$_('workshop_banned')}</div>
		{/if}
		<details bind:open={expanded}>
			<summary>
				<span class="details-label">{$_('workshop_details')}<ChevronDown size=".9rem"/></span>
				<span class="dates">
					{#if item.timeCreated}<span>{$_('created')}: {#key item.timeCreated}<Timestamp unix={item.timeCreated}/>{/key}</span>{/if}
					{#if item.timeUpdated}<span>{$_('updated')}: {#key item.timeUpdated}<Timestamp unix={item.timeUpdated}/>{/key}</span>{/if}
				</span>
			</summary>
			<div class="expanded">
				<dl>
					<div><dt>{$_('workshop_settings_visibility')}</dt><dd>{item.visibility ? $_('workshop_visibility_' + item.visibility) : $_('workshop_unknown')}</dd></div>
					<div><dt>{$_('workshop_published_size')}</dt><dd>{item.fileSize == null ? $_('workshop_unknown') : formatSize(item.fileSize, $locale)}</dd></div>
					{#if estimatedSize != null}
						<div title={$_('workshop_estimated_size_help')}><dt>{$_('workshop_estimated_size')}</dt><dd>{formatSize(estimatedSize, $locale)}</dd></div>
					{/if}
					{#each metrics as metric}
						<div><dt>{$_('workshop_stat_' + metric)}</dt><dd>{item.dead || item[metric] == null ? $_('workshop_unknown') : item[metric].toLocaleString(numberLocale)}</dd></div>
					{/each}
				</dl>
				<p>{$_('workshop_stats_help')}</p>
			</div>
		</details>
	</section>
{/if}

<style>
	.workshop-stats { flex: 0 0 auto; border: 1px solid #414141; border-radius: 4px; background: #292929; font-size: .85em; }
	summary { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: .4rem 1rem; padding: .6rem .75rem; cursor: pointer; list-style: none; border-radius: 4px; }
	summary:hover { background: #313131; }
	summary::-webkit-details-marker { display: none; }
	summary:focus-visible { outline: 2px solid #127cff; outline-offset: -2px; }
	.details-label, .dates, .banned { display: flex; align-items: center; gap: .6rem; }
	.details-label { color: #fff; font-weight: 600; }
	.dates { flex-wrap: wrap; color: #aaa; gap: .4rem 1rem; }
	.dates > span { white-space: nowrap; }
	.expanded { max-height: 28vh; overflow: auto; padding: 0 .75rem .6rem; border-top: 1px solid #414141; }
	dl { display: grid; grid-template-columns: repeat(auto-fit, minmax(8rem, 1fr)); gap: .9rem 1rem; margin: .8rem 0; }
	dt { color: #aaa; margin-bottom: .25rem; }
	dd { margin: 0; color: #fff; font-size: 1.1em; font-variant-numeric: tabular-nums; }
	p { color: #aaa; margin: .5rem 0 0; font-size: .9em; line-height: 1.4; }
	.banned { color: #fff; padding: .6rem .75rem; background: var(--error-dark); border-radius: 3px 3px 0 0; }
</style>
