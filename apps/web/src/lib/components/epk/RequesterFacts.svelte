<script module lang="ts">
	import { roleLabel } from '#lib/capabilities.ts';
	import type { RequesterAbout } from '#lib/epk.ts';

	/** How many genres a DJ's line names before it stops. */
	const GENRES_SHOWN = 3;

	/** One line per thing the requester's profile says they do, and one per venue they're in. */
	export function requesterFacts({ promoter, performer, venues }: RequesterAbout): string[] {
		const cities = promoter?.cities?.map((place) => place.city) ?? [];
		const genres = performer?.genres?.slice(0, GENRES_SHOWN) ?? [];

		return [
			...(promoter ? [cities.length ? `Promoter in ${cities.join(', ')}` : 'Promoter'] : []),
			...(performer ? [genres.length ? `DJ · ${genres.join(', ')}` : 'DJ'] : []),
			...venues.map((venue) => `${roleLabel(venue.role)} at ${venue.name}${venue.locality ? `, ${venue.locality}` : ''}`)
		];
	}
</script>

<script lang="ts">
	let { about }: { about: RequesterAbout } = $props();

	const facts = $derived(requesterFacts(about));
</script>

{#if facts.length}
	<ul class="flex flex-wrap gap-x-3 gap-y-0.5 text-small text-fg-muted">
		{#each facts as fact (fact)}<li>{fact}</li>{/each}
	</ul>
{/if}
