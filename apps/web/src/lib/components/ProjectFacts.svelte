<script lang="ts">
	import { feeLabel } from '#lib/format.ts';
	import { FORMATS, labelFor, SET_TYPES } from '#lib/taxonomy/options.ts';
	import type { ProjectProfile } from '#lib/types.ts';

	/** What a promoter checks first: fee, slots, formats, set length, room sizes and travel. */
	let { profile }: { profile: ProjectProfile } = $props();

	const facts = $derived(
		[
			{ term: 'Fee', value: profile.fee ? feeLabel(profile) : null },
			{ term: 'Slots', value: profile.setTypes?.length ? profile.setTypes.map((value) => labelFor(SET_TYPES, value)).join(', ') : null },
			{ term: 'Plays on', value: profile.formats?.length ? profile.formats.map((value) => labelFor(FORMATS, value)).join(', ') : null },
			{ term: 'Set length', value: profile.setLength ? `${profile.setLength.minMinutes ?? '?'}–${profile.setLength.maxMinutes ?? '?'} minutes` : null },
			{ term: 'Rooms', value: profile.capacity ? `${profile.capacity.min ?? 0}–${profile.capacity.max ?? 'any'} people` : null },
			{
				term: 'Travel',
				value: profile.travel ? `${profile.travel.radiusKm ? `Up to ${profile.travel.radiusKm} km` : 'Local only'}${profile.travel.international ? ', international' : ''}` : null,
				note: profile.travel?.notes
			},
			{ term: 'Playing since', value: profile.activeSince ? String(profile.activeSince) : null }
		].filter((fact) => fact.value)
	);
</script>

{#if facts.length}
	<dl class="m-0 flex flex-col">
		{#each facts as fact (fact.term)}
			<div class="flex flex-col gap-0.5 border-b border-line py-2.5 first:pt-0 last:border-b-0 last:pb-0">
				<dt class="eyebrow text-fg-subtle">{fact.term}</dt>
				<dd class="m-0 text-small">{fact.value}</dd>
				{#if fact.note}<dd class="m-0 text-label text-fg-muted">{fact.note}</dd>{/if}
			</div>
		{/each}
	</dl>
{/if}
