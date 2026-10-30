<script lang="ts">
	import { enhance } from '$app/forms';
	import { ScrollText } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import SpacePeople from '#lib/components/SpacePeople.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDate } from '#lib/format.ts';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const title = $derived(`${data.space.kind.charAt(0).toUpperCase()}${data.space.kind.slice(1)}`);
</script>

<svelte:head><title>{title} · Spaces · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} {title} lede="Started {formatDate(data.space.createdAt)}.">
		{#snippet actions()}
			<a class="btn-secondary" href="/admin/audit?q={encodeURIComponent(data.space.uri)}"><ScrollText class="size-4" />Audit log</a>
			<form method="POST" action={data.space.listed ? '?/unlist' : '?/list'} use:enhance>
				<button class="btn-secondary w-full">{data.space.listed ? 'Take out of the directory' : 'List in the directory'}</button>
			</form>
		{/snippet}
	</SectionHeader>
	<div class="flex flex-col gap-1">
		<p class="flex flex-wrap items-center gap-2 text-small text-fg-muted"><StatusPill status={data.space.listed ? 'listed' : 'unlisted'} />Started by <Handle account={data.createdBy} /></p>
		<p class="mono break-all text-fg-subtle">{data.space.uri}</p>
	</div>
	<FormMessage {form} />
	<div class="grid grid-cols-[minmax(0,1fr)] items-start gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
		<SpacePeople people={data.people} pending={data.pending} roles={data.roles} />
		<form method="POST" action="?/add" class="card flex flex-col gap-4 p-5" use:enhance>
			<h2 class="text-emphasis font-semibold">Add someone</h2>
			<div><label class="label" for="subject">Handle or DID</label><input id="subject" name="subject" class="input" required placeholder="alice.bsky.social" /></div>
			<div>
				<label class="label" for="role">Role</label>
				<select id="role" name="role" class="input capitalize">{#each data.roles as role (role)}<option value={role}>{role}</option>{/each}</select>
			</div>
			<label class="flex items-center gap-2 text-small text-fg-soft"><input type="checkbox" class="checkbox" name="hidden" />Hidden from everyone outside it</label>
			<button class="btn-primary w-full">Add</button>
		</form>
	</div>
</div>
