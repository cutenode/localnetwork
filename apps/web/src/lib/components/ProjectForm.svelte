<script lang="ts">
	import type { Snippet } from 'svelte';
	import { enhance } from '$app/forms';
	import { Plus, Trash2 } from '@lucide/svelte';
	import { FEE_TIERS, FORMATS, LINK_KINDS, PROJECT_KINDS, SET_TYPES } from '#lib/taxonomy/options.ts';
	import { djSetLabel } from '#lib/format.ts';
	import type { DjSetView, ProjectFormValues, ProjectLink, ProjectMix, StrongRef } from '#lib/types.ts';
	import Avatar from './Avatar.svelte';
	import GenreInput from './GenreInput.svelte';
	import PlacePicker from './PlacePicker.svelte';

	let {
		values = {},
		action = '',
		submitLabel = 'Submit for review',
		showDetails = true,
		showMessage = false,
		avatarUrl = null,
		geocoder = true,
		sets,
		children,
		footer
	}: {
		values?: ProjectFormValues;
		action?: string;
		submitLabel?: string;
		showDetails?: boolean;
		showMessage?: boolean;
		avatarUrl?: string | null;
		geocoder?: boolean;
		/** The managers' published sets (null when they can't be loaded), for a picker on each mix. No picker without. */
		sets?: Promise<DjSetView[] | null>;
		children?: Snippet;
		footer?: Snippet;
	} = $props();

	let submitting = $state(false);

	const initialLinks = () => {
		const list = values.links ?? [];

		return list.length ? list.map((link) => ({ ...link })) : [{ kind: 'soundcloud', url: '' }];
	};

	// Mix rows have stable keys: each holds a file input, which must stay with its own mix.
	let nextMixKey = 0;

	const initialMixes = () => {
		const list = values.mixes ?? [];

		return (list.length ? list : [{ title: '', url: '' }]).map((mix) => ({ ...mix, key: nextMixKey++ }));
	};

	let links = $state<ProjectLink[]>(initialLinks());

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

<!-- Floated, a legend leaves the fieldset's border alone and lays out like the other flex items. -->
{#snippet legend(title: string, description: string)}
	<legend class="eyebrow float-left -mb-3.5 text-fg-subtle">{title}</legend>
	<p class="text-small text-fg-muted">{description}</p>
{/snippet}

{#snippet remove(label: string, onclick: () => void)}
	<button type="button" class="grid size-11 shrink-0 place-items-center rounded-lg text-fg-muted hover:bg-raised hover:text-fg" aria-label={label} {onclick}><Trash2 class="size-4" /></button>
{/snippet}

<form
	method="POST"
	{action}
	enctype="multipart/form-data"
	class="flex flex-col gap-6"
	use:enhance={() => {
		submitting = true;
		return async ({ update }) => {
			await update({ reset: false });
			submitting = false;
		};
	}}
>
	<fieldset class="card flex flex-col gap-5 p-5 sm:p-6">
		{@render legend('The basics', 'How promoters will see the project in search results.')}
		<div class="grid gap-4 sm:grid-cols-[1fr_14rem]">
			<div>
				<label class="label" for="name">Project name</label>
				<input id="name" class="input" name="name" value={v('name')} required maxlength="64" />
			</div>
			<div>
				<label class="label" for="kind">Type</label>
				<select id="kind" class="input" name="kind">
					{#each PROJECT_KINDS as option (option.value)}<option value={option.value} selected={v('kind') === option.value}>{option.label}</option>{/each}
				</select>
			</div>
		</div>
		<div class="grid gap-4 sm:grid-cols-[1fr_10rem]">
			<div>
				<label class="label" for="tagline">Tagline</label>
				<input id="tagline" class="input" name="tagline" value={v('tagline')} maxlength="140" placeholder="Hypnotic, rolling techno for late rooms" />
			</div>
			<div>
				<label class="label" for="pronouns">Pronouns <span class="font-normal text-fg-subtle">(optional)</span></label>
				<input id="pronouns" class="input" name="pronouns" value={v('pronouns')} maxlength="24" />
			</div>
		</div>
		<div>
			<label class="label" for="genres">Genres</label>
			<GenreInput value={v('genres')} />
			<p class="hint">Up to 12. Pick from suggestions so promoters searching for related sounds find you too.</p>
		</div>
		<div>
			<label class="label" for="bio">Bio</label>
			<textarea id="bio" class="input min-h-36 py-2.5" name="bio" maxlength="30000">{v('bio')}</textarea>
		</div>
		<div class="flex items-start gap-4">
			<Avatar src={avatarUrl} name={v('name') || 'New project'} size="xl" />
			<div class="min-w-0 flex-1">
				<label class="label" for="avatar">Photo or logo</label>
				<input id="avatar" type="file" name="avatar" accept="image/png,image/jpeg,image/webp" class="block w-full text-small text-fg-muted" />
				<p class="hint">PNG, JPEG, or WebP, up to 1 MB.</p>
				{#if avatarUrl}
					<label class="mt-3 flex items-center gap-2.5 text-small text-fg-soft"><input type="checkbox" class="checkbox" name="removeAvatar" /> Remove the current image</label>
				{/if}
			</div>
		</div>
	</fieldset>

	<fieldset class="card flex flex-col gap-5 p-5 sm:p-6">
		{@render legend('Where', 'Local matters: promoters search by distance from their venue.')}
		<PlacePicker {values} {geocoder} />
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
	</fieldset>

	<fieldset class="card flex flex-col gap-5 p-5 sm:p-6">
		{@render legend('Sets and setup', 'Helps match the project to the right slot.')}
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
	</fieldset>

	<fieldset class="card flex flex-col gap-5 p-5 sm:p-6">
		{@render legend('Fees', 'The tier is public and used for budget matching. Amounts are optional.')}
		<div class="grid grid-cols-2 gap-4 sm:grid-cols-4">
			<div>
				<label class="label" for="feeTier">Tier</label>
				<select id="feeTier" class="input" name="feeTier">
					<option value="">Not listed</option>
					{#each FEE_TIERS as option (option.value)}<option value={option.value} selected={v('feeTier') === option.value}>{option.label}</option>{/each}
				</select>
			</div>
			<div>
				<label class="label" for="feeCurrency">Currency</label>
				<input id="feeCurrency" class="input uppercase" name="feeCurrency" value={v('feeCurrency')} maxlength="3" placeholder="USD" />
			</div>
			<div>
				<label class="label" for="feeMin">From</label>
				<input id="feeMin" class="input" type="number" min="0" name="feeMin" value={v('feeMin')} />
			</div>
			<div>
				<label class="label" for="feeMax">Up to</label>
				<input id="feeMax" class="input" type="number" min="0" name="feeMax" value={v('feeMax')} />
			</div>
		</div>
	</fieldset>

	<fieldset class="card flex flex-col gap-5 p-5 sm:p-6">
		{@render legend('Links and mixes', 'SoundCloud, Mixcloud, and YouTube mixes can be played right on the profile.')}
		<div class="flex flex-col gap-2">
			<span class="label mb-0">Links</span>
			{#each links as link, index (index)}
				<!-- On a phone the URL takes its own row under the type and the remove button. -->
				<div class="flex flex-wrap gap-2 sm:flex-nowrap">
					<select class="input min-w-0 flex-1 sm:w-44 sm:flex-none" name="linkKind" bind:value={link.kind} aria-label="Link type">
						{#each LINK_KINDS as option (option.value)}<option value={option.value}>{option.label}</option>{/each}
					</select>
					<input class="input order-last sm:order-none sm:min-w-0 sm:flex-1" name="linkUrl" bind:value={link.url} placeholder="https://" aria-label="Link URL" />
					<!-- No field edits this here; posting it back keeps it on save. -->
					<input type="hidden" name="linkLabel" value={link.label ?? ''} />
					{@render remove('Remove link', () => (links = links.filter((_, i) => i !== index)))}
				</div>
			{/each}
			{#if links.length < 12}
				<button type="button" class="btn-ghost btn-sm self-start" onclick={() => (links = [...links, { kind: 'website', url: '' }])}><Plus class="size-4" />Add link</button>
			{/if}
		</div>
		<div class="flex flex-col gap-2">
			<span class="label mb-0">Mixes</span>
			{#each mixes as mix (mix.key)}
				<div class="flex flex-col gap-2">
					<div class="flex flex-wrap gap-2 sm:flex-nowrap">
						<input class="input min-w-0 flex-1 sm:w-56 sm:flex-none" name="mixTitle" bind:value={mix.title} placeholder="Title" aria-label="Mix title" />
						<input class="input order-last sm:order-none sm:min-w-0 sm:flex-1" name="mixUrl" bind:value={mix.url} placeholder="https://soundcloud.com/…" aria-label="Mix URL" />
						{@render remove('Remove mix', () => (mixes = mixes.filter((m) => m.key !== mix.key)))}
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
	</fieldset>

	{#if showDetails}
		<fieldset class="card flex flex-col gap-5 p-5 sm:p-6">
			{@render legend('Private booking details', "Only signed-in members see these. They're stored in a private atproto space, not your public repo.")}
			<div class="grid gap-4 sm:grid-cols-2">
				<div>
					<label class="label" for="bookingEmail">Booking email</label>
					<input id="bookingEmail" class="input" type="email" name="bookingEmail" value={v('bookingEmail')} />
				</div>
				<div>
					<label class="label" for="bookingPhone">Booking phone</label>
					<input id="bookingPhone" class="input" name="bookingPhone" value={v('bookingPhone')} />
				</div>
			</div>
			<div>
				<label class="label" for="rateNotes">Rates</label>
				<textarea id="rateNotes" class="input py-2.5" name="rateNotes">{v('rateNotes')}</textarea>
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div>
					<label class="label" for="techRider">Tech rider</label>
					<textarea id="techRider" class="input min-h-28 py-2.5" name="techRider" placeholder="2× CDJ-3000, DJM-900NXS2, booth monitors">{v('techRider')}</textarea>
				</div>
				<div>
					<label class="label" for="hospitality">Hospitality</label>
					<textarea id="hospitality" class="input min-h-28 py-2.5" name="hospitality">{v('hospitality')}</textarea>
				</div>
			</div>
		</fieldset>
	{/if}

	{#if children}{@render children()}{/if}

	{#if showMessage}
		<div class="card p-5 sm:p-6">
			<label class="label" for="message">Note to moderators <span class="font-normal text-fg-subtle">(optional)</span></label>
			<textarea id="message" class="input py-2.5" name="message" maxlength="2000">{v('message')}</textarea>
		</div>
	{/if}

	<div class="flex flex-wrap items-center justify-end gap-3">
		{#if footer}{@render footer()}{/if}
		<button class="btn-primary" disabled={submitting}>{submitting ? 'Saving…' : submitLabel}</button>
	</div>
</form>
