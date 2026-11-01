<script lang="ts">
	import { Plus } from '@lucide/svelte';
	import { djSetLabel } from '#lib/format.ts';
	import { FEE_TIERS, FORMATS, SET_TYPES } from '#lib/taxonomy/options.ts';
	import type { DjSetView, FeeFormRow, ProjectFormValues, ProjectMix, StrongRef } from '#lib/types.ts';
	import GenreInput from '../GenreInput.svelte';
	import FormSection from './FormSection.svelte';
	import RemoveRow from './RemoveRow.svelte';

	/**
	 * What promoters need to book a performer (`section.defs#performer`): sound, travel, sets and setup, fees, and
	 * mixes. A project shows the same fields. `sets` are the account's published sets for a picker on each mix (null
	 * when they can't be loaded); without them there's no picker.
	 */
	let { values = {}, sets }: { values?: ProjectFormValues; sets?: Promise<DjSetView[] | null> } = $props();

	// Mix rows have stable keys: each holds a file input, which must stay with its own mix.
	let nextMixKey = 0;

	const initialMixes = () => {
		const list = values.mixes ?? [];

		return (list.length ? list : [{ title: '', url: '' }]).map((mix) => ({ ...mix, key: nextMixKey++ }));
	};

	// Fee rows have stable keys too: their inputs aren't bound, so each must stay with its own row.
	let nextFeeKey = 0;

	const emptyFee = (): FeeFormRow & { key: number } => ({ label: '', tier: '', currency: '', minAmount: '', maxAmount: '', key: nextFeeKey++ });

	const initialFees = () => {
		const list = values.fees ?? [];

		return list.length ? list.map((fee) => ({ ...fee, key: nextFeeKey++ })) : [emptyFee()];
	};

	let fees = $state<(FeeFormRow & { key: number })[]>(initialFees());

	let mixes = $state<(ProjectMix & { key: number })[]>(initialMixes());

	type Picker = { state: 'loading' } | { state: 'ready'; sets: DjSetView[] } | { state: 'error' };

	let picker = $state<Picker>({ state: 'loading' });

	$effect(() => {
		if (!sets) return;
		const pending = sets;
		let current = true;
		picker = { state: 'loading' };

		void (async () => {
			const next: Picker = await pending.then(
				(found) => (found ? { state: 'ready', sets: found } : { state: 'error' }),
				() => ({ state: 'error' })
			);

			if (current) picker = next;
		})();

		return () => (current = false);
	});

	/** A set's picker value: its URI and the CID of the version picked (read by setRef in forms/project.ts). */
	const setValue = (ref: StrongRef) => `${ref.uri} ${ref.cid}`;

	/** The picker's choices for a mix linked to `linked`, which stays one even while sets load or can't. */
	function setChoices(linked: StrongRef | undefined) {
		const choices = picker.state === 'ready' ? picker.sets.map((set) => ({ value: setValue(set), label: djSetLabel(set) })) : [];

		if (linked && !choices.some((choice) => choice.value === setValue(linked))) {
			const latest = picker.state === 'ready' ? picker.sets.find((set) => set.uri === linked.uri) : undefined;
			const label = latest ? `${djSetLabel(latest)} (earlier version)` : picker.state === 'ready' ? 'Linked set (no longer published)' : 'Linked set';
			choices.unshift({ value: setValue(linked), label });
		}

		return choices;
	}

	const pickerStatus = $derived(
		picker.state === 'loading' ? 'Loading sets…' : picker.state === 'error' ? "Couldn't load sets" : picker.sets.length === 0 ? 'No published sets yet' : null
	);

	function pickSet(mix: ProjectMix, value: string) {
		const [uri, cid] = value.split(' ');
		mix.set = uri && cid ? { uri, cid } : undefined;
	}

	const v = (key: string) => String(values[key] ?? '');

	const has = (key: 'formats' | 'setTypes', option: string) => (values[key] ?? []).includes(option);
</script>

<FormSection title="Sound" description="How promoters find you: a one-line pitch and the genres you play.">
	<div>
		<label class="label" for="tagline">Tagline</label>
		<input id="tagline" class="input" name="tagline" value={v('tagline')} maxlength="140" placeholder="Hypnotic, rolling techno for late rooms" />
	</div>
	<div>
		<label class="label" for="genres">Genres</label>
		<GenreInput value={v('genres')} />
		<p class="hint">Up to 12. Pick from suggestions so promoters searching for related sounds find you too.</p>
	</div>
</FormSection>

<FormSection title="Travel" description="How far you'll go for a booking.">
	<div class="grid gap-4 sm:grid-cols-[14rem_1fr] sm:items-end">
		<div>
			<label class="label" for="travelRadiusKm">Will travel up to (km)</label>
			<input id="travelRadiusKm" class="input" type="number" name="travelRadiusKm" min="0" max="20000" value={v('travelRadiusKm')} />
		</div>
		<label class="flex min-h-11 items-center gap-2.5 text-small text-fg-soft">
			<input type="checkbox" class="checkbox" name="international" checked={values.international} /> Open to international bookings
		</label>
	</div>
	<div>
		<label class="label" for="travelNotes">Travel notes</label>
		<input id="travelNotes" class="input" name="travelNotes" value={v('travelNotes')} maxlength="300" placeholder="Has a car; needs a hotel for gigs over 200 km" />
	</div>
</FormSection>

<FormSection title="Sets and setup" description="Helps match you to the right slot.">
	<fieldset>
		<legend class="label">Set types</legend>
		<div class="flex flex-wrap gap-2">
			{#each SET_TYPES as option (option.value)}
				<label class="chip-toggle">
					<input type="checkbox" class="sr-only" name="setTypes" value={option.value} checked={has('setTypes', option.value)} />{option.label}
				</label>
			{/each}
		</div>
	</fieldset>
	<fieldset>
		<legend class="label">Formats</legend>
		<div class="flex flex-wrap gap-2">
			{#each FORMATS as option (option.value)}
				<label class="chip-toggle">
					<input type="checkbox" class="sr-only" name="formats" value={option.value} checked={has('formats', option.value)} />{option.label}
				</label>
			{/each}
		</div>
	</fieldset>
	<div class="grid grid-cols-2 gap-4 md:grid-cols-5">
		<div>
			<label class="label" for="setMin">Shortest set (min)</label>
			<input id="setMin" class="input" type="number" name="setMin" min="15" max="720" value={v('setMin')} />
		</div>
		<div>
			<label class="label" for="setMax">Longest set (min)</label>
			<input id="setMax" class="input" type="number" name="setMax" min="15" max="720" value={v('setMax')} />
		</div>
		<div>
			<label class="label" for="capacityMin">Smallest room</label>
			<input id="capacityMin" class="input" type="number" name="capacityMin" min="0" value={v('capacityMin')} />
		</div>
		<div>
			<label class="label" for="capacityMax">Biggest room</label>
			<input id="capacityMax" class="input" type="number" name="capacityMax" min="0" value={v('capacityMax')} />
		</div>
		<div>
			<label class="label" for="activeSince">Playing since</label>
			<input id="activeSince" class="input" type="number" name="activeSince" min="1950" max="2100" value={v('activeSince')} placeholder="2018" />
		</div>
	</div>
</FormSection>

<FormSection title="Fees" description="One level per kind of booking. Tiers are public and used for budget matching. Amounts are optional.">
	<div class="flex flex-col gap-4">
		{#each fees as fee (fee.key)}
			<div class="flex flex-col gap-2">
				<!-- On a phone the label takes its own row under the tier and the remove button. -->
				<div class="flex flex-wrap gap-2 sm:flex-nowrap">
					<select class="input min-w-0 flex-1 sm:w-44 sm:flex-none" name="feeTier" aria-label="Fee tier">
						<option value="">Pick a tier</option>
						{#each FEE_TIERS as option (option.value)}<option value={option.value} selected={fee.tier === option.value}>{option.label}</option>{/each}
					</select>
					<input class="input order-last sm:order-none sm:min-w-0 sm:flex-1" name="feeLabel" value={fee.label} maxlength="64" placeholder="What it covers, like club nights" aria-label="What this fee covers" />
					<RemoveRow label="Remove fee" onclick={() => (fees = fees.filter((f) => f.key !== fee.key))} />
				</div>
				<div class="grid grid-cols-[5.5rem_1fr_1fr] gap-2 sm:grid-cols-[11rem_1fr_1fr] sm:pr-13">
					<input class="input uppercase" name="feeCurrency" value={fee.currency} maxlength="3" placeholder="USD" aria-label="Currency" />
					<input class="input" type="number" min="0" name="feeMin" value={fee.minAmount} placeholder="From" aria-label="From" />
					<input class="input" type="number" min="0" name="feeMax" value={fee.maxAmount} placeholder="Up to" aria-label="Up to" />
				</div>
			</div>
		{/each}
		<button type="button" class="btn-ghost btn-sm self-start" onclick={() => (fees = [...fees, emptyFee()])}><Plus class="size-4" />Add fee</button>
	</div>
</FormSection>

<FormSection title="Mixes" description="SoundCloud, Mixcloud, and YouTube mixes can be played right on the page.">
	<div class="flex flex-col gap-2">
		{#each mixes as mix (mix.key)}
			<div class="flex flex-col gap-2">
				<div class="flex flex-wrap gap-2 sm:flex-nowrap">
					<input class="input min-w-0 flex-1 sm:w-56 sm:flex-none" name="mixTitle" bind:value={mix.title} placeholder="Title" aria-label="Mix title" />
					<input class="input order-last sm:order-none sm:min-w-0 sm:flex-1" name="mixUrl" bind:value={mix.url} placeholder="https://soundcloud.com/…" aria-label="Mix URL" />
					<RemoveRow label="Remove mix" onclick={() => (mixes = mixes.filter((m) => m.key !== mix.key))} />
				</div>
				<div class="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-small text-fg-muted">
					<input type="hidden" name="mixTracklistCid" value={mix.tracklist?.ref.$link ?? ''} />
					<!-- No fields edit these here; posting them back keeps them on save. -->
					<input type="hidden" name="mixDuration" value={mix.durationMinutes ?? ''} />
					<input type="hidden" name="mixYear" value={mix.year ?? ''} />
					{#if mix.tracklist}
						<span class="text-good">Tracklist attached</span>
						<button type="button" class="btn-ghost btn-sm" onclick={() => (mix.tracklist = undefined)}>Remove</button>
						<span>or replace it:</span>
					{:else}
						<span>Tracklist (optional):</span>
					{/if}
					<input type="file" name="mixTracklist" accept=".json,application/json" class="max-w-full text-small" aria-label="Tracklist for {mix.title || 'this mix'}" />
				</div>
				{#if sets}
					<div class="flex flex-wrap items-center gap-x-3 gap-y-1 pl-1 text-base">
						<label class="muted" for="mix-set-{mix.key}">Set (optional):</label>
						<select id="mix-set-{mix.key}" class="input min-w-0 grow basis-56 truncate pr-10" name="mixSet" onchange={(event) => pickSet(mix, event.currentTarget.value)}>
							<option value="" selected={!mix.set}>None</option>
							{#each setChoices(mix.set) as choice (choice.value)}<option value={choice.value} selected={mix.set && choice.value === setValue(mix.set)}>{choice.label}</option>{/each}
							{#if pickerStatus}<option disabled>{pickerStatus}</option>{/if}
						</select>
					</div>
				{/if}
			</div>
		{/each}
		{#if mixes.length < 12}
			<button type="button" class="btn-ghost btn-sm self-start" onclick={() => (mixes = [...mixes, { title: '', url: '', key: nextMixKey++ }])}><Plus class="size-4" />Add mix</button>
		{/if}
		<p class="hint">
			A tracklist is a mix report from locallibrary (<code class="font-mono text-fg-muted">locallibrary mix … --export set.mix.json</code>): what you played, when, and how you mixed it, shown
			under the mix. <a class="link" href="/mixes/preview" target="_blank">Preview a file</a> before you add it.
		</p>
		{#if sets}
			<p class="hint">A set is one you published with locallibrary (<a class="link" href="/tools/locallibrary/docs/account#publishing"><code>locallibrary sets publish</code></a>): when and where you played it.</p>
		{/if}
	</div>
	<div>
		<label class="label" for="highlights">Highlights</label>
		<textarea id="highlights" class="input min-h-24 py-2.5" name="highlights" placeholder="One per line: residencies, festivals, releases">{v('highlights')}</textarea>
		<p class="hint">Up to 12.</p>
	</div>
	<div>
		<label class="label" for="lookingFor">Looking for</label>
		<textarea id="lookingFor" class="input py-2.5" name="lookingFor" maxlength="5000" placeholder="Warm-up slots at queer parties, daytime outdoor events…">{v('lookingFor')}</textarea>
	</div>
</FormSection>
