<script lang="ts">
	import { Plus } from '@lucide/svelte';
	import { LINK_KINDS } from '#lib/taxonomy/options.ts';
	import type { ProjectLink } from '#lib/types.ts';
	import RemoveRow from './RemoveRow.svelte';

	/**
	 * Links as rows of a type and a URL (`linkKind`, `linkUrl`, `linkLabel`, which parseBasics reads), up to the 12 the
	 * basics hold. With none saved it starts with one empty row of `blank`'s type.
	 */
	let { saved = [], blank = 'soundcloud' }: { saved?: ProjectLink[]; blank?: string } = $props();

	const initialLinks = () => (saved.length ? saved.map((link) => ({ ...link })) : [{ kind: blank, url: '' }]);

	let links = $state<ProjectLink[]>(initialLinks());
</script>

<div class="flex flex-col gap-2">
	{#each links as link, index (index)}
		<!-- On a phone, or with large text, the URL takes its own row under the type and the remove button. -->
		<div class="flex flex-wrap gap-2">
			<select class="input min-w-0 flex-1 sm:w-44 sm:flex-none" name="linkKind" aria-label="Link type" onchange={(event) => (link.kind = event.currentTarget.value)}>
				{#each LINK_KINDS as option (option.value)}<option value={option.value} selected={link.kind === option.value}>{option.label}</option>{/each}
			</select>
			<input class="input order-last sm:order-none sm:min-w-0 sm:flex-[1_1_12rem]" name="linkUrl" bind:value={link.url} placeholder="https://" aria-label="Link URL" />
			<!-- No field edits this here; posting it back keeps it on save. -->
			<input type="hidden" name="linkLabel" value={link.label ?? ''} />
			<RemoveRow label="Remove link" onclick={() => (links = links.filter((_, i) => i !== index))} />
		</div>
	{/each}
	{#if links.length < 12}
		<button type="button" class="btn-ghost btn-sm self-start" onclick={() => (links = [...links, { kind: 'website', url: '' }])}><Plus class="size-4" />Add link</button>
	{/if}
</div>
