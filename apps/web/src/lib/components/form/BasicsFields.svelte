<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Plus } from '@lucide/svelte';
	import { LINK_KINDS } from '#lib/taxonomy/options.ts';
	import type { ProjectFormValues, ProjectLink } from '#lib/types.ts';
	import Avatar from '../Avatar.svelte';
	import PlacePicker from '../PlacePicker.svelte';
	import FormSection from './FormSection.svelte';
	import RemoveRow from './RemoveRow.svelte';

	/**
	 * What every profile and project has (`section.defs#basics`): a name, pronouns, a bio, a picture, where they're
	 * based, and links. `nameAside` sits beside the name, like a project's kind. A project has to say where it is; a
	 * person doesn't.
	 */
	let {
		values = {},
		avatarUrl = null,
		geocoder = true,
		nameLabel = 'Name',
		description,
		placeRequired = false,
		nameAside
	}: {
		values?: ProjectFormValues;
		avatarUrl?: string | null;
		geocoder?: boolean;
		nameLabel?: string;
		description: string;
		placeRequired?: boolean;
		nameAside?: Snippet;
	} = $props();

	const initialLinks = () => {
		const list = values.links ?? [];

		return list.length ? list.map((link) => ({ ...link })) : [{ kind: 'soundcloud', url: '' }];
	};

	let links = $state<ProjectLink[]>(initialLinks());

	const v = (key: string) => String(values[key] ?? '');
</script>

<FormSection title="The basics" {description}>
	<div class={['grid gap-4', nameAside && 'sm:grid-cols-[1fr_14rem]']}>
		<div>
			<label class="label" for="name">{nameLabel}</label>
			<input id="name" class="input" name="name" value={v('name')} required maxlength="64" />
		</div>
		{#if nameAside}{@render nameAside()}{/if}
	</div>
	<div class="sm:max-w-60">
		<label class="label" for="pronouns">Pronouns <span class="font-normal text-fg-subtle">(optional)</span></label>
		<input id="pronouns" class="input" name="pronouns" value={v('pronouns')} maxlength="24" />
	</div>
	<div>
		<label class="label" for="bio">Bio</label>
		<textarea id="bio" class="input min-h-36 py-2.5" name="bio" maxlength="30000">{v('bio')}</textarea>
	</div>
	<div class="flex items-start gap-4">
		<Avatar src={avatarUrl} name={v('name') || 'New'} size="xl" />
		<div class="min-w-0 flex-1">
			<label class="label" for="avatar">Photo or logo</label>
			<input id="avatar" type="file" name="avatar" accept="image/png,image/jpeg,image/webp" class="block w-full text-small text-fg-muted" />
			<p class="hint">PNG, JPEG, or WebP, up to 1 MB.</p>
			{#if avatarUrl}
				<label class="mt-3 flex items-center gap-2.5 text-small text-fg-soft"><input type="checkbox" class="checkbox" name="removeAvatar" /> Remove the current image</label>
			{/if}
		</div>
	</div>
</FormSection>

<FormSection title="Where" description="Local matters: promoters search by distance from their venue.">
	<PlacePicker {values} {geocoder} required={placeRequired} />
</FormSection>

<FormSection title="Links" description="Where people find you elsewhere.">
	<div class="flex flex-col gap-2">
		{#each links as link, index (index)}
			<!-- On a phone the URL takes its own row under the type and the remove button. -->
			<div class="flex flex-wrap gap-2 sm:flex-nowrap">
				<select class="input min-w-0 flex-1 sm:w-44 sm:flex-none" name="linkKind" bind:value={link.kind} aria-label="Link type">
					{#each LINK_KINDS as option (option.value)}<option value={option.value}>{option.label}</option>{/each}
				</select>
				<input class="input order-last sm:order-none sm:min-w-0 sm:flex-1" name="linkUrl" bind:value={link.url} placeholder="https://" aria-label="Link URL" />
				<!-- No field edits this here; posting it back keeps it on save. -->
				<input type="hidden" name="linkLabel" value={link.label ?? ''} />
				<RemoveRow label="Remove link" onclick={() => (links = links.filter((_, i) => i !== index))} />
			</div>
		{/each}
		{#if links.length < 12}
			<button type="button" class="btn-ghost btn-sm self-start" onclick={() => (links = [...links, { kind: 'website', url: '' }])}><Plus class="size-4" />Add link</button>
		{/if}
	</div>
</FormSection>
