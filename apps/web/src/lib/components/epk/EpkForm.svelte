<script lang="ts">
	import { Check, Circle } from '@lucide/svelte';
	import type { Snippet } from 'svelte';
	import { enhance } from '$app/forms';
	import { type EpkFormValues, photoSize, pressChecks, type Visibility } from '#lib/epk.ts';
	import { formFields } from '#lib/form-fields.svelte.ts';
	import { control } from '#lib/ui/Field.svelte';
	import TextField from '#lib/ui/TextField.svelte';
	import AccessMatrix from './AccessMatrix.svelte';

	/**
	 * A press kit's form: what it's called and says, who can see the rest, which of the project's
	 * mixes and which photos show to everyone or only on request, and the private part itself.
	 */
	let {
		values,
		action,
		photoBase,
		editHref,
		submitLabel,
		actions
	}: {
		values: EpkFormValues;
		action?: string;
		/** Where saved photos are served from, for the thumbnails. */
		photoBase: string | null;
		/** The project's edit page, for adding mixes. */
		editHref: string;
		submitLabel: string;
		actions?: Snippet;
	} = $props();

	let saving = $state(false);

	const PLACES: { value: Visibility; label: string }[] = [
		{ value: 'public', label: 'Everyone' },
		{ value: 'private', label: 'On request' },
		{ value: 'hidden', label: 'Not shown' }
	];

	const emptyMix = (): EpkFormValues['extraMixes'][number] => ({ title: '', url: '', visibility: 'private' });

	const fields = $derived(
		formFields({
			...values,
			// The extra mixes on file, then two empty rows to add more.
			extraMixes: [...values.extraMixes, emptyMix(), emptyMix()],
			quotes: Array.from({ length: 5 }, (_, index) => values.quotes[index] ?? { text: '', source: '' })
		})
	);

	const checks = $derived(pressChecks(values.photos.filter((photo) => photo.visibility !== 'hidden')));

	const SIZE_LABELS = { press: 'Press ready', web: 'Web size', small: 'Small' } as const;
</script>

<form
	method="POST"
	{action}
	enctype="multipart/form-data"
	class="flex flex-col gap-10"
	use:enhance={() => {
		saving = true;

		return async ({ update }) => {
			await update({ reset: false });
			saving = false;
		};
	}}
>
	<input type="hidden" name="cid" value={values.cid} />
	<section aria-labelledby="epk-basics" class="flex flex-col gap-4">
		<h2 id="epk-basics" class="m-0 text-emphasis font-semibold">The public part</h2>
		<TextField id="title" name="title" label="Name" hint="What this press kit is for, like Clubs or Festivals 2027." required maxlength={100} value={values.title} />
		<TextField id="headline" name="headline" label="Headline" hint="One line under your name." maxlength={300} value={values.headline} />
		<div class="flex flex-col gap-1.5">
			<label for="bio" class="text-label font-medium text-fg-soft">Short bio</label>
			<textarea id="bio" name="bio" rows="4" maxlength="1000" class={control} aria-describedby="bio-hint" bind:value={fields.bio}></textarea>
			<p id="bio-hint" class="m-0 text-label text-fg-subtle">Five or six sentences anyone can read. Keep it current and plain; promoters skip hype.</p>
		</div>
	</section>

	<AccessMatrix access={values.access} preset={values.preset} />

	<section aria-labelledby="epk-mixes" class="flex flex-col gap-3">
		<h2 id="epk-mixes" class="m-0 text-emphasis font-semibold">Mixes</h2>
		{#if values.mixes.length}
			<p class="m-0 text-small text-fg-muted">Promoters judge you on your mixes first. Pick which ones anyone can play, and keep promos for people you let in.</p>
			<ul class="m-0 flex list-none flex-col p-0">
				{#each fields.mixes as mix (mix.url)}
					<li class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 border-t border-line py-3 last:border-b">
						<span class="min-w-0 text-small font-semibold">{mix.title}</span>
						<fieldset class="m-0 flex flex-wrap gap-1.5 border-0 p-0">
							<legend class="sr-only">Who can play {mix.title}</legend>
							{#each PLACES as place (place.value)}
								<label class="chip-toggle"><input type="radio" class="sr-only" name="mixVisibility.{mix.url}" value={place.value} bind:group={mix.visibility} />{place.label}</label>
							{/each}
						</fieldset>
					</li>
				{/each}
			</ul>
		{:else}
			<p class="m-0 text-small text-fg-muted">This project has no mixes yet. <a class="link" href={editHref}>Add some to the project</a>, or add ones only this press kit has below.</p>
		{/if}
		<fieldset class="m-0 flex min-w-0 flex-col gap-3 border-0 p-0">
			<legend class="mb-1 text-label font-medium text-fg-soft">Mixes only in this press kit</legend>
			<p class="m-0 text-label text-fg-subtle">Like an unreleased promo. They aren't added to your profile.</p>
			{#each fields.extraMixes as mix, index (index)}
				<div class="grid gap-2 sm:grid-cols-[1fr_1fr_auto] sm:items-center">
					<input name="extraMixTitle" aria-label="Extra mix {index + 1} name" placeholder="Name" maxlength="120" bind:value={mix.title} class={control} />
					<input name="extraMixUrl" type="url" aria-label="Extra mix {index + 1} address" placeholder="https://" bind:value={mix.url} class={control} />
					<fieldset class="m-0 flex flex-wrap gap-1.5 border-0 p-0">
						<legend class="sr-only">Who can play extra mix {index + 1}</legend>
						{#each PLACES as place (place.value)}
							<label class="chip-toggle"><input type="radio" class="sr-only" name="extraMixVisibility.{index}" value={place.value} bind:group={mix.visibility} />{place.value === 'hidden' ? 'Remove' : place.label}</label>
						{/each}
					</fieldset>
				</div>
			{/each}
		</fieldset>
	</section>

	<section aria-labelledby="epk-photos" class="flex flex-col gap-4">
		<div class="flex flex-col gap-1">
			<h2 id="epk-photos" class="m-0 text-emphasis font-semibold">Photos</h2>
			<p class="m-0 text-small text-fg-muted">Press wants 3 to 5 recent photos from one shoot, credited, in every orientation so nothing gets cropped.</p>
		</div>
		<ul class="m-0 flex list-none flex-wrap gap-x-5 gap-y-1.5 p-0 text-label" aria-label="Press photo checklist">
			{#each checks as check (check.label)}
				<li class="inline-flex items-center gap-1.5 {check.done ? 'text-good' : 'text-fg-muted'}">
					{#if check.done}<Check class="size-3.5" aria-hidden="true" />{:else}<Circle class="size-3.5" aria-hidden="true" />{/if}{check.label}
				</li>
			{/each}
		</ul>
		{#if values.photos.length}
			<ul class="m-0 grid list-none gap-4 p-0 sm:grid-cols-2">
				{#each fields.photos as photo, index (photo.cid)}
					<li class="card flex flex-col gap-3 p-3">
						<input type="hidden" name="photoCid" value={photo.cid} />
						{#if photoBase}<img src="{photoBase}/{photo.cid}" alt={photo.alt} class="aspect-[4/3] w-full rounded-lg bg-raised object-cover" />{/if}
						<p class="m-0 flex flex-wrap gap-x-3 font-mono text-caption text-fg-muted">
							{#if photo.width && photo.height}<span>{photo.width} × {photo.height}</span><span>{SIZE_LABELS[photoSize(photo.width, photo.height) ?? 'small']}</span>{/if}
						</p>
						<fieldset class="m-0 flex flex-wrap gap-1.5 border-0 p-0">
							<legend class="sr-only">Who sees photo {index + 1}</legend>
							{#each PLACES as place (place.value)}
								<label class="chip-toggle"><input type="radio" class="sr-only" name="photoVisibility.{index}" value={place.value} bind:group={photo.visibility} />{place.value === 'hidden' ? 'Remove' : place.label}</label>
							{/each}
						</fieldset>
						<TextField id="photo-alt-{index}" name="photoAlt" label="Description" maxlength={1000} value={photo.alt} />
						<TextField id="photo-credit-{index}" name="photoCredit" label="Photographer" maxlength={200} value={photo.credit} />
					</li>
				{/each}
			</ul>
		{/if}
		<div class="flex flex-col gap-3 rounded-xl border border-dashed border-line-strong p-4">
			<div class="flex flex-col gap-1.5">
				<label for="newPhotos" class="text-label font-medium text-fg-soft">Add photos</label>
				<input id="newPhotos" name="newPhotos" type="file" accept="image/png,image/jpeg,image/webp" multiple class="text-small" aria-describedby="newPhotos-hint" />
				<p id="newPhotos-hint" class="m-0 text-label text-fg-subtle">PNG, JPEG or WebP, up to 5 MB each. A 3,000 px JPEG fits.</p>
			</div>
			<fieldset class="m-0 flex flex-wrap items-center gap-1.5 border-0 p-0">
				<legend class="mb-1.5 text-label font-medium text-fg-soft">Who sees them</legend>
				<label class="chip-toggle"><input type="radio" class="sr-only" name="newPhotoVisibility" value="public" checked />Everyone</label>
				<label class="chip-toggle"><input type="radio" class="sr-only" name="newPhotoVisibility" value="private" />On request</label>
			</fieldset>
			<TextField id="newPhotoCredit" name="newPhotoCredit" label="Photographer" maxlength={200} />
		</div>
	</section>

	<section aria-labelledby="epk-private" class="flex flex-col gap-4">
		<div class="flex flex-col gap-1">
			<h2 id="epk-private" class="m-0 text-emphasis font-semibold">On request</h2>
			<p class="m-0 text-small text-fg-muted">Only the project's managers and the people they let in see this.</p>
		</div>
		<div class="flex flex-col gap-1.5">
			<label for="longBio" class="text-label font-medium text-fg-soft">Full bio</label>
			<textarea id="longBio" name="longBio" rows="8" maxlength="5000" class={control} bind:value={fields.longBio}></textarea>
		</div>
		<fieldset class="m-0 flex min-w-0 flex-col gap-3 border-0 p-0">
			<legend class="mb-1 text-label font-medium text-fg-soft">Press quotes</legend>
			{#each fields.quotes as quote, index (index)}
				<div class="grid gap-2 sm:grid-cols-[2fr_1fr_1fr]">
					<input name="quoteText" aria-label="Quote {index + 1}" placeholder="What they said" maxlength="500" bind:value={quote.text} class={control} />
					<input name="quoteSource" aria-label="Who said quote {index + 1}" placeholder="Who said it" maxlength="200" bind:value={quote.source} class={control} />
					<input name="quoteUrl" type="url" aria-label="Link for quote {index + 1}" placeholder="https://" bind:value={quote.url} class={control} />
				</div>
			{/each}
		</fieldset>
		<div class="grid gap-4 sm:grid-cols-2">
			<TextField id="email" name="email" type="email" label="Booking email" maxlength={320} value={values.email} />
			<TextField id="phone" name="phone" type="tel" label="Phone" maxlength={64} value={values.phone} />
		</div>
		<TextField id="contactNote" name="contactNote" label="Contact note" hint="Like who to ask for, or when you reply." maxlength={500} value={values.contactNote} />
		<div class="flex flex-col gap-1.5">
			<label for="techRider" class="text-label font-medium text-fg-soft">Tech rider</label>
			<textarea id="techRider" name="techRider" rows="4" maxlength="2000" class={control} bind:value={fields.techRider}></textarea>
		</div>
	</section>

	<div class="flex flex-wrap items-center gap-3 border-t border-line pt-6">
		<button class="btn-primary" disabled={saving}>{saving ? 'Saving…' : submitLabel}</button>
		{@render actions?.()}
	</div>
</form>
