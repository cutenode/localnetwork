<script lang="ts">
	import type { Snippet } from 'svelte';
	import { formFields } from '#lib/form-fields.svelte.ts';
	import type { ProjectFormValues } from '#lib/types.ts';
	import Avatar from '../Avatar.svelte';
	import PlacePicker from '../PlacePicker.svelte';
	import FormSection from './FormSection.svelte';
	import LinkFields from './LinkFields.svelte';

	/**
	 * What every profile and project has (`section.defs#basics`): a name, pronouns, a bio, a picture, where they're
	 * based, and links. `nameAside` sits beside the name, like a project's kind. A project has to say where it is; a
	 * person doesn't. A person's pronouns come from their Bluesky profile, so they pass `pronouns` (empty for none),
	 * which shows them without a field; a project edits its own.
	 */
	let {
		values = {},
		avatarUrl = null,
		geocoder = true,
		nameLabel = 'Name',
		description,
		placeRequired = false,
		nameAside,
		pronouns
	}: {
		values?: ProjectFormValues;
		avatarUrl?: string | null;
		geocoder?: boolean;
		nameLabel?: string;
		description: string;
		placeRequired?: boolean;
		nameAside?: Snippet;
		pronouns?: string;
	} = $props();

	const fields = $derived(formFields(values));

	const v = (key: string) => String(values[key] ?? '');
</script>

<FormSection title="The basics" {description}>
	<div class={['grid gap-4', nameAside && 'sm:grid-cols-[minmax(0,1fr)_14rem]']}>
		<div>
			<label class="label" for="name">{nameLabel}</label>
			<input id="name" class="input" name="name" bind:value={fields.name} required maxlength="64" />
		</div>
		{#if nameAside}{@render nameAside()}{/if}
	</div>
	{#if pronouns === undefined}
		<div class="sm:max-w-60">
			<label class="label" for="pronouns">Pronouns <span class="font-normal text-fg-subtle">(optional)</span></label>
			<input id="pronouns" class="input" name="pronouns" bind:value={fields.pronouns} maxlength="24" />
		</div>
	{:else}
		<div>
			<p class="label">Pronouns</p>
			<p class="m-0 text-body text-fg">{pronouns || 'None set'}</p>
			<p class="hint">Change them in your Bluesky profile.</p>
		</div>
	{/if}
	<div>
		<label class="label" for="bio">Bio</label>
		<textarea id="bio" class="input min-h-36 py-2.5" name="bio" maxlength="30000" bind:value={fields.bio}></textarea>
	</div>
	<div class="flex items-start gap-4">
		<Avatar src={avatarUrl} name={v('name') || 'New'} size="xl" />
		<div class="min-w-0 flex-1">
			<label class="label" for="avatar">Photo or logo</label>
			<input id="avatar" type="file" name="avatar" accept="image/png,image/jpeg,image/webp" class="block w-full text-small text-fg-muted" />
			<p class="hint">PNG, JPEG, or WebP, up to 1 MB.</p>
			{#if avatarUrl}
				<label class="mt-3 flex items-center gap-2.5 text-small text-fg-soft"><input type="checkbox" class="checkbox" name="removeAvatar" bind:checked={() => fields.removeAvatar === 'on', (on) => (fields.removeAvatar = on ? 'on' : '')} /> Remove the current image</label>
			{/if}
		</div>
	</div>
</FormSection>

<FormSection title="Where" description="Local matters: promoters search by distance from their venue.">
	<PlacePicker {values} {geocoder} required={placeRequired} />
</FormSection>

<FormSection title="Links" description="Where people find you elsewhere.">
	<LinkFields saved={values.links} />
</FormSection>
