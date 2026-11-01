<script module lang="ts">
	import { feeLabel, feeLevels } from '#lib/format.ts';
	import { FORMATS, labelFor, SET_TYPES } from '#lib/taxonomy/options.ts';
	import type { PerformerSection, ProjectProfile } from '#lib/types.ts';

	/** A DJ's performer section, or a project's profile, which has the same fields. */
	type Performer = PerformerSection & Pick<ProjectProfile, 'fee'>;

	/** What a promoter checks first: fee, slots, formats, set length, room sizes and travel, those on file. */
	export function performerFacts(profile: Performer) {
		// One line per fee level, led by what it covers.
		const fees = feeLevels(profile).map((fee) => (fee.label ? `${fee.label}: ${feeLabel(fee)}` : feeLabel(fee)));

		return [
			{ term: fees.length > 1 ? 'Fees' : 'Fee', value: fees.length ? fees : null },
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
		].filter((fact) => fact.value);
	}
</script>

<script lang="ts">
	let { performer }: { performer: Performer } = $props();

	const facts = $derived(performerFacts(performer));
</script>

{#if facts.length}
	<dl class="m-0 flex flex-col">
		{#each facts as fact (fact.term)}
			<div class="flex flex-col gap-0.5 border-b border-line py-2.5 first:pt-0 last:border-b-0 last:pb-0">
				<dt class="eyebrow text-fg-subtle">{fact.term}</dt>
				{#each [fact.value].flat() as line, index (index)}<dd class="m-0 text-small">{line}</dd>{/each}
				{#if fact.note}<dd class="m-0 text-label text-fg-muted">{fact.note}</dd>{/if}
			</div>
		{/each}
	</dl>
{/if}
