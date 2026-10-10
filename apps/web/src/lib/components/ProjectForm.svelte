<script lang="ts">
	import type { Snippet } from 'svelte';
	import { enhance } from '$app/forms';
	import { Plus, Trash2 } from '@lucide/svelte';
	import { FEE_TIERS, FORMATS, LINK_KINDS, PROJECT_KINDS, SET_TYPES } from '#lib/taxonomy/options.ts';
	import type { ProjectFormValues, ProjectLink, ProjectMix } from '#lib/types.ts';
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

	const v = (key: string) => String(values[key] ?? '');

	const has = (key: 'formats' | 'setTypes', option: string) => (values[key] ?? []).includes(option);
</script>

<form
	method="POST"
	{action}
	enctype="multipart/form-data"
	class="space-y-8"
	use:enhance={() => {
		submitting = true;
		return async ({ update }) => {
			await update({ reset: false });
			submitting = false;
		};
	}}
>
	<section class="card card-pad space-y-5">
		<div>
			<h2 class="section-title">The basics</h2>
			<p class="muted text-sm">How promoters will see the project in search results.</p>
		</div>
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
				<label class="label" for="pronouns">Pronouns <span class="muted">(optional)</span></label>
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
			<textarea id="bio" class="input min-h-36" name="bio" maxlength="30000">{v('bio')}</textarea>
		</div>
		<div class="flex items-center gap-4">
			<Avatar src={avatarUrl} name={v('name') || 'New project'} size="lg" />
			<div class="flex-1">
				<label class="label" for="avatar">Photo or logo</label>
				<input id="avatar" type="file" name="avatar" accept="image/png,image/jpeg,image/webp" class="block w-full text-base" />
				<p class="hint">PNG, JPEG, or WebP, up to 1 MB.</p>
				{#if avatarUrl}
					<label class="mt-2 flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" name="removeAvatar" /> Remove the current image</label>
				{/if}
			</div>
		</div>
	</section>

	<section class="card card-pad space-y-5">
		<div>
			<h2 class="section-title">Where</h2>
			<p class="muted text-sm">Local matters: promoters search by distance from their venue.</p>
		</div>
		<PlacePicker {values} {geocoder} />
		<div class="grid gap-4 sm:grid-cols-3">
			<div>
				<label class="label" for="travelRadiusKm">Will travel up to (km)</label>
				<input id="travelRadiusKm" class="input" type="number" name="travelRadiusKm" min="0" max="20000" value={v('travelRadiusKm')} />
			</div>
			<label class="flex items-center gap-2 self-end pb-2 text-sm">
				<input type="checkbox" class="checkbox" name="international" checked={values.international} /> Open to international bookings
			</label>
		</div>
		<div>
			<label class="label" for="travelNotes">Travel notes</label>
			<input id="travelNotes" class="input" name="travelNotes" value={v('travelNotes')} maxlength="300" placeholder="Has a car; needs a hotel for gigs over 200 km" />
		</div>
	</section>

	<section class="card card-pad space-y-5">
		<div>
			<h2 class="section-title">Sets and setup</h2>
			<p class="muted text-sm">Helps match the project to the right slot.</p>
		</div>
		<fieldset>
			<legend class="label">Set types</legend>
			<div class="flex flex-wrap gap-2">
				{#each SET_TYPES as option (option.value)}
					<label class="chip chip-toggle gap-2 py-0.5">
						<input type="checkbox" class="checkbox" name="setTypes" value={option.value} checked={has('setTypes', option.value)} />{option.label}
					</label>
				{/each}
			</div>
		</fieldset>
		<fieldset>
			<legend class="label">Formats</legend>
			<div class="flex flex-wrap gap-2">
				{#each FORMATS as option (option.value)}
					<label class="chip chip-toggle gap-2 py-0.5">
						<input type="checkbox" class="checkbox" name="formats" value={option.value} checked={has('formats', option.value)} />{option.label}
					</label>
				{/each}
			</div>
		</fieldset>
		<div class="grid gap-4 sm:grid-cols-5">
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
	</section>

	<section class="card card-pad space-y-5">
		<div>
			<h2 class="section-title">Fees</h2>
			<p class="muted text-sm">The tier is public and used for budget matching. Amounts are optional.</p>
		</div>
		<div class="grid gap-4 sm:grid-cols-4">
			<div class="sm:col-span-1">
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
	</section>

	<section class="card card-pad space-y-5">
		<div>
			<h2 class="section-title">Links and mixes</h2>
			<p class="muted text-sm">SoundCloud, Mixcloud, and YouTube mixes can be played right on the profile.</p>
		</div>
		<div class="space-y-2">
			<span class="label">Links</span>
			{#each links as link, index (index)}
				<div class="flex gap-2">
					<select class="input w-44" name="linkKind" bind:value={link.kind} aria-label="Link type">
						{#each LINK_KINDS as option (option.value)}<option value={option.value}>{option.label}</option>{/each}
					</select>
					<input class="input" name="linkUrl" bind:value={link.url} placeholder="https://" aria-label="Link URL" />
					<!-- No field edits this here; posting it back keeps it on save. -->
					<input type="hidden" name="linkLabel" value={link.label ?? ''} />
					<button type="button" class="btn-ghost px-2" aria-label="Remove link" onclick={() => (links = links.filter((_, i) => i !== index))}><Trash2 class="size-4" /></button>
				</div>
			{/each}
			{#if links.length < 12}
				<button type="button" class="btn-ghost btn-sm" onclick={() => (links = [...links, { kind: 'website', url: '' }])}><Plus class="size-3" />Add link</button>
			{/if}
		</div>
		<div class="space-y-2">
			<span class="label">Mixes</span>
			{#each mixes as mix (mix.key)}
				<div class="space-y-1">
					<div class="flex gap-2">
						<input class="input w-56" name="mixTitle" bind:value={mix.title} placeholder="Title" aria-label="Mix title" />
						<input class="input" name="mixUrl" bind:value={mix.url} placeholder="https://soundcloud.com/…" aria-label="Mix URL" />
						<button type="button" class="btn-ghost px-2" aria-label="Remove mix" onclick={() => (mixes = mixes.filter((m) => m.key !== mix.key))}><Trash2 class="size-4" /></button>
					</div>
					<div class="flex flex-wrap items-center gap-x-3 gap-y-1 pl-1 text-base">
						<input type="hidden" name="mixTracklistCid" value={mix.tracklist?.ref.$link ?? ''} />
						<!-- No fields edit these here; posting them back keeps them on save. -->
						<input type="hidden" name="mixDuration" value={mix.durationMinutes ?? ''} />
						<input type="hidden" name="mixYear" value={mix.year ?? ''} />
						{#if mix.tracklist}
							<span class="text-ansi-bright-green">Tracklist attached</span>
							<button type="button" class="btn-ghost btn-sm" onclick={() => (mix.tracklist = undefined)}>Remove</button>
							<span class="muted">or replace it:</span>
						{:else}
							<span class="muted">Tracklist (optional):</span>
						{/if}
						<input type="file" name="mixTracklist" accept=".json,application/json" class="max-w-full text-base" aria-label="Tracklist for {mix.title || 'this mix'}" />
					</div>
				</div>
			{/each}
			{#if mixes.length < 12}
				<button type="button" class="btn-ghost btn-sm" onclick={() => (mixes = [...mixes, { title: '', url: '', key: nextMixKey++ }])}><Plus class="size-3" />Add mix</button>
			{/if}
			<p class="hint">
				A tracklist is a mix report from locallibrary (<code>locallibrary mix … --export set.mix.json</code>): what you played, when, and how you mixed it, shown
				under the mix. <a class="link" href="/mixes/preview" target="_blank">Preview a file</a> before you add it.
			</p>
		</div>
		<div>
			<label class="label" for="highlights">Highlights</label>
			<textarea id="highlights" class="input min-h-24" name="highlights" placeholder="One per line: residencies, festivals, releases">{v('highlights')}</textarea>
			<p class="hint">Up to 12.</p>
		</div>
		<div>
			<label class="label" for="lookingFor">Looking for</label>
			<textarea id="lookingFor" class="input" name="lookingFor" maxlength="5000" placeholder="Warm-up slots at queer parties, daytime outdoor events…">{v('lookingFor')}</textarea>
		</div>
	</section>

	{#if showDetails}
		<section class="card card-pad space-y-5">
			<div>
				<h2 class="section-title">Private booking details</h2>
				<p class="muted text-sm">Only signed-in members see these. They're stored in a private atproto space, not your public repo.</p>
			</div>
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
				<textarea id="rateNotes" class="input" name="rateNotes">{v('rateNotes')}</textarea>
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div>
					<label class="label" for="techRider">Tech rider</label>
					<textarea id="techRider" class="input min-h-28" name="techRider" placeholder="2× CDJ-3000, DJM-900NXS2, booth monitors">{v('techRider')}</textarea>
				</div>
				<div>
					<label class="label" for="hospitality">Hospitality</label>
					<textarea id="hospitality" class="input min-h-28" name="hospitality">{v('hospitality')}</textarea>
				</div>
			</div>
		</section>
	{/if}

	{#if children}{@render children()}{/if}

	{#if showMessage}
		<section class="card card-pad space-y-3">
			<label class="label" for="message">Note to moderators <span class="muted">(optional)</span></label>
			<textarea id="message" class="input" name="message" maxlength="2000">{v('message')}</textarea>
		</section>
	{/if}

	<div class="flex flex-wrap items-center justify-end gap-3">
		{#if footer}{@render footer()}{/if}
		<button class="btn-accent" disabled={submitting}>{submitting ? 'Saving…' : submitLabel}</button>
	</div>
</form>
