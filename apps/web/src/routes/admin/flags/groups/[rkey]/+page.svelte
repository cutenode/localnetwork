<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import { formFields } from '#lib/form-fields.svelte.ts';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const group = $derived(data.group);

	const fields = $derived(
		formFields({
			name: group.name,
			description: group.description ?? '',
			members: group.members.map((member) => member.handle ?? member.did).join('\n')
		})
	);
</script>

<svelte:head><title>{group.name} · Flag groups · Local Network</title></svelte:head>

<div class="flex flex-col gap-6">
	<a href="/admin/flags/groups" class="inline-flex items-center gap-1.5 text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />Flag groups</a>
	<SectionHeader level={1} title={group.name} lede={group.flags.length > 0 ? `Used by ${group.flags.join(', ')}` : 'No flags use this group.'} />
	<FormMessage {form} />
	<form method="POST" action="?/save" class="card flex max-w-2xl flex-col gap-4 p-5" use:enhance={() => ({ update }) => update({ reset: false })}>
		<div><label class="label" for="name">Name</label><input id="name" name="name" class="input" required maxlength="64" bind:value={fields.name} /></div>
		<div><label class="label" for="description">Description</label><input id="description" name="description" class="input" maxlength="300" bind:value={fields.description} /></div>
		<div>
			<label class="label" for="members">Accounts</label>
			<textarea id="members" name="members" class="input min-h-40 py-2.5 font-mono text-small" aria-describedby="members-hint" placeholder="alice.bsky.social" bind:value={fields.members}></textarea>
			<p id="members-hint" class="hint">One handle or DID per line.</p>
		</div>
		<button class="btn-primary self-start">Save</button>
	</form>
	{#if group.flags.length === 0}
		<form method="POST" action="?/delete" use:enhance={({ cancel }) => { if (!confirm('Delete this group?')) cancel(); }}>
			<button class="btn-danger btn-sm">Delete group</button>
		</form>
	{/if}
</div>
