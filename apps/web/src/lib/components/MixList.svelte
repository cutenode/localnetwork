<script lang="ts">
	import { ListMusic, Play } from '@lucide/svelte';
	import type { DjSetView, ProjectMix } from '#lib/types.ts';
	import LinkedSet from './LinkedSet.svelte';

	/**
	 * `tracklistHref` links mixes that have a tracklist to their page (by position, from 0). `sets` are the linked
	 * sets the directory has, by URI, or null when it couldn't say.
	 */
	let {
		mixes,
		tracklistHref,
		sets
	}: { mixes: ProjectMix[]; tracklistHref?: (index: number) => string; sets?: Promise<Map<string, DjSetView> | null> } = $props();

	/** The open player, by position and link, since records written elsewhere can repeat a link. */
	let open = $state<string | null>(null);

	function embedUrl(url: string): string | null {
		try {
			const parsed = new URL(url);

			if (parsed.hostname.endsWith('soundcloud.com')) {
				return `https://w.soundcloud.com/player/?url=${encodeURIComponent(url)}&color=%23ff55ff&auto_play=true&visual=false`;
			}

			if (parsed.hostname.endsWith('mixcloud.com')) {
				return `https://player-widget.mixcloud.com/widget/iframe/?hide_cover=1&feed=${encodeURIComponent(parsed.pathname)}`;
			}

			const youtube = parsed.hostname.includes('youtu.be') ? parsed.pathname.slice(1) : parsed.searchParams.get('v');

			if (youtube && (parsed.hostname.includes('youtube.com') || parsed.hostname.includes('youtu.be'))) {
				return `https://www.youtube-nocookie.com/embed/${encodeURIComponent(youtube)}`;
			}
		} catch {
			return null;
		}

		return null;
	}
</script>

<ul class="space-y-3">
	{#each mixes as mix, index (index)}
		{@const embed = embedUrl(mix.url)}
		{@const id = `${index} ${mix.url}`}
		<li class="border-2 border-ansi-blue p-3">
			<div class="flex items-center justify-between gap-3">
				<a class="link truncate" href={mix.url} rel="noopener noreferrer nofollow" target="_blank">{mix.title}</a>
				<span class="flex shrink-0 gap-2">
					{#if mix.tracklist && tracklistHref}
						<a class="btn-ghost btn-sm" href={tracklistHref(index)}><ListMusic class="size-3" />Tracklist</a>
					{/if}
					{#if embed}
						<button type="button" class="btn-secondary btn-sm" onclick={() => (open = open === id ? null : id)}>
							<Play class="size-3" />{open === id ? 'Close' : 'Play'}
						</button>
					{/if}
				</span>
			</div>
			{#if mix.set && sets}<LinkedSet linked={mix.set} {sets} class="mt-1" />{/if}
			{#if embed && open === id}
				<iframe title={mix.title} src={embed} class="mt-3 h-40 w-full border-2 border-ansi-blue" loading="lazy" allow="autoplay; encrypted-media" referrerpolicy="strict-origin-when-cross-origin"></iframe>
			{/if}
		</li>
	{/each}
</ul>
