<script lang="ts">
	import { formatShowDate, pluralize } from '#lib/format.ts';
	import type { DjSetView, StrongRef } from '#lib/types.ts';

	/** A mix's linked set, looked up in `sets` (the linked sets the directory has, by URI, or null when it couldn't say). */
	let { linked, sets, class: className = '' }: { linked: StrongRef; sets: Promise<Map<string, DjSetView> | null>; class?: string } = $props();
</script>

<p class="muted text-sm {className}">
	{#await sets}
		<span aria-hidden="true" class="text-ansi-dark">░░░░░░░░░░░░ · ░░░░░░░░</span><span class="sr-only">Loading the set</span>
	{:then found}
		{@const set = found?.get(linked.uri)}
		{#if set}
			{@const place = [set.title && set.venue, set.location?.locality].filter(Boolean).join(', ')}
			{@const details = [formatShowDate(set.date), place, pluralize(set.trackCount, 'track')].filter(Boolean).join(' · ')}
			Set: <a class="link" href={set.uri}>{set.title ?? set.venue ?? 'Untitled set'}</a> · {details}
			{#if set.cid !== linked.cid}<span class="text-ansi-yellow">· Changed since it was linked</span>{/if}
		{:else if found}
			Linked set no longer published
		{:else}
			<!-- The directory couldn't say, so the link is all there is to show. -->
			Set: <a class="link" href={linked.uri}>linked set</a>
		{/if}
	{/await}
</p>
