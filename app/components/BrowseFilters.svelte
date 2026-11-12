<script>
	import { _ } from 'svelte-i18n';
	import Check from '@lucide/svelte/icons/check';
	import X from '@lucide/svelte/icons/x';
	import { ADDON_TAGS, ADDON_TYPES, tagLabel } from '../browse.js';

	export let query;
	export let tagOptions = [];
	export let onChange;

	$: tags = query.kind === 'addon' ? ADDON_TAGS : tagOptions;
	$: sources = ['everyone', 'friends', 'followed', 'favorites', ...(query.author ? ['author'] : [])];

	function toggleType(type, checked) {
		onChange({ types: checked ? [...query.types, type] : query.types.filter(value => value !== type) });
	}

	const same = (left, right) => left.toLowerCase() === right.toLowerCase();
	$: stateOf = tag => (query.tags.some(value => same(value, tag)) ? 'include' : query.excludedTags.some(value => same(value, tag)) ? 'exclude' : 'off');

	function cycleTag(tag) {
		const state = stateOf(tag);
		const tagsWithout = query.tags.filter(value => !same(value, tag));
		const excludedWithout = query.excludedTags.filter(value => !same(value, tag));
		if (state === 'off') onChange({ tags: [...tagsWithout, tag], excludedTags: excludedWithout });
		else if (state === 'include') onChange({ tags: tagsWithout, excludedTags: [...excludedWithout, tag] });
		else onChange({ tags: tagsWithout, excludedTags: excludedWithout });
	}

	function setRange(field, side, value) {
		onChange({ [field]: { ...query[field], [side]: value } });
	}
</script>

<aside class="filters scrollbar" aria-label={$_('workshop_filters')}>
	<fieldset>
		<legend>{$_('browse_search_in')}</legend>
		{#each ['any', 'title', 'description'] as scope}
			<label>
				<input type="radio" name="browse-text-scope" checked={query.textScope === scope} on:change={() => onChange({ textScope: scope })}/>
				{$_('browse_scope_' + scope)}
			</label>
		{/each}
		<label>
			<input type="checkbox" checked={query.exactPhrase} on:change={event => onChange({ exactPhrase: event.currentTarget.checked })}/>
			{$_('browse_exact_phrase')}
		</label>
		{#if query.kind !== 'collection'}<p class="help">{$_('browse_requires_help')}</p>{/if}
	</fieldset>

	<fieldset>
		<legend>{$_('browse_source')}</legend>
		{#each sources as source}
			<label>
				<input type="radio" name="browse-source" checked={query.source === source} on:change={() => onChange({ source })}/>
				{source === 'author' ? $_('browse_by',{ values: { name: query.author.name ?? query.author.id } }) : $_('browse_source_' + source)}
			</label>
		{/each}
	</fieldset>

	{#if query.kind === 'addon'}
		<fieldset>
			<legend>{$_('browse_types')}</legend>
			{#each ADDON_TYPES as type}
				<label>
					<input type="checkbox" checked={query.types.includes(type)} on:change={event => toggleType(type, event.currentTarget.checked)}/>
					<span class="swatch" data-tag={type.toLowerCase()}></span>
					{$_('addon_types.' + type)}
				</label>
			{/each}
		</fieldset>
	{/if}

	{#if query.kind !== 'collection' && tags.length}
		<fieldset>
			<legend>{$_('browse_tags')}</legend>
			<p class="help">{$_('browse_tags_help')}</p>
			<div class="tags">
				{#each tags as tag}
					{@const state = stateOf(tag)}
					<button type="button" class="tag-chip {state}" data-tag={tag.toLowerCase()} aria-label={$_('browse_tag_' + state, { values: { tag: tagLabel(tag) } })} on:click={() => cycleTag(tag)}>
						{#if state === 'include'}<Check size=".85rem"/>{:else if state === 'exclude'}<X size=".85rem"/>{/if}
						{tagLabel(tag)}
					</button>
				{/each}
			</div>
			<label>
				<input type="checkbox" checked={query.matchAll} disabled={query.tags.length < 2} on:change={event => onChange({ matchAll: event.currentTarget.checked })}/>
				{$_('browse_match_all')}
			</label>
		</fieldset>
	{/if}

	{#each ['created', 'updated'] as field}
		<fieldset>
			<legend>{$_('browse_' + field)}</legend>
			<div class="range dates">
				<label class="field">
					<span>{$_('browse_from')}</span>
					<input type="date" value={query[field].start} max={query[field].end || undefined} on:change={event => setRange(field, 'start', event.currentTarget.value)}/>
				</label>
				<label class="field">
					<span>{$_('browse_to')}</span>
					<input type="date" value={query[field].end} min={query[field].start || undefined} on:change={event => setRange(field, 'end', event.currentTarget.value)}/>
				</label>
			</div>
		</fieldset>
	{/each}

	<fieldset>
		<legend>{$_('browse_more_filters')}</legend>
		<p class="help">{$_('browse_in_app_help')}</p>
		{#if query.kind !== 'collection'}
			<div class="range">
				<label class="field">
					<span>{$_('browse_size_min')}</span>
					<input type="number" min="0" step="any" value={query.sizeMin} on:change={event => onChange({ sizeMin: event.currentTarget.value })}/>
				</label>
				<label class="field">
					<span>{$_('browse_size_max')}</span>
					<input type="number" min="0" step="any" value={query.sizeMax} on:change={event => onChange({ sizeMax: event.currentTarget.value })}/>
				</label>
			</div>
		{/if}
		<label class="field">
			<span>{$_('browse_min_subscribers')}</span>
			<input type="number" min="0" step="1" value={query.minSubscribers} on:change={event => onChange({ minSubscribers: event.currentTarget.value })}/>
		</label>
		<label class="field">
			<span>{$_('browse_min_rating')}</span>
			<input type="number" min="0" max="100" step="1" value={query.minRating} on:change={event => onChange({ minRating: event.currentTarget.value })}/>
		</label>
		<label class="field">
			<span>{$_('browse_status')}</span>
			<select value={query.status} on:change={event => onChange({ status: event.currentTarget.value })}>
				{#each ['any', 'subscribed', 'not_subscribed'] as status}
					<option value={status}>{$_('browse_status_' + status)}</option>
				{/each}
			</select>
		</label>
	</fieldset>
</aside>

<style>
	.filters {
		flex: 0 0 16rem;
		overflow-x: hidden;
		overflow-y: auto;
		padding: 0 1rem 1.5rem 1.5rem;
		border-inline-end: 1px solid var(--border);
	}
	fieldset {
		margin: 0 0 1rem;
		padding: 0;
		border: 0;
		min-width: 0;
	}
	legend {
		margin-bottom: .4rem;
		padding: 0;
		font-weight: 600;
	}
	label {
		display: flex;
		align-items: center;
		gap: .5rem;
		padding: .3rem 0;
		cursor: pointer;
	}
	input[type='checkbox'], input[type='radio'] {
		accent-color: var(--neutral);
	}
	.help {
		margin: 0 0 .5rem;
		color: var(--text-muted);
		font-size: .8rem;
		line-height: 1.4;
	}
	.tags {
		display: flex;
		flex-wrap: wrap;
		gap: .35rem;
		margin-bottom: .3rem;
	}
	.tag-chip {
		gap: .25rem;
		padding: .25rem .55rem;
		font-size: .85rem;
		cursor: pointer;
		transition: background-color .12s, border-color .12s;
	}
	.tag-chip.off:hover {
		background: color-mix(in srgb, var(--tag-shade) 32%, transparent);
	}
	.tag-chip.include:hover {
		filter: brightness(1.1);
	}
	.tag-chip.exclude:hover {
		background: color-mix(in srgb, var(--error) 15%, transparent);
	}
	.swatch {
		flex-shrink: 0;
		width: .6rem;
		height: .6rem;
		border-radius: 50%;
		background: var(--tag, var(--text-muted));
	}
	.range {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: .5rem;
	}
	.range.dates {
		grid-template-columns: 1fr;
	}
	.field {
		flex-direction: column;
		align-items: stretch;
		gap: .25rem;
		cursor: default;
	}
	.field span {
		color: var(--text-soft);
		font-size: .8rem;
	}
	.field input, .field select {
		box-sizing: border-box;
		width: 100%;
		min-width: 0;
		padding: .4rem .5rem;
		border: 1px solid var(--border-strong);
		border-radius: 4px;
		background: var(--control);
		color: var(--text);
		font: inherit;
		color-scheme: inherit;
	}
	button:focus-visible, input:focus-visible, select:focus-visible {
		outline: 2px solid var(--text);
		outline-offset: 2px;
	}
</style>
