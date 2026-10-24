<script lang="ts">
	import { enhance } from '$app/forms';
	import { ArrowLeft, ArrowUpRight } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { timeAgo } from '#lib/format.ts';
	import Card from '#lib/ui/Card.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const project = $derived(data.project);
</script>

<svelte:head><title>{project.profile.name} · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	<div>
		<a href="/admin/projects" class="inline-flex items-center gap-1.5 text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />Projects</a>
		<div class="mt-3 flex flex-wrap items-center gap-x-4 gap-y-3">
			<h1 class="text-title font-semibold">{project.profile.name}</h1>
			<StatusPill status={project.status} />
			<a href="/projects/{project.slug}" class="btn-secondary btn-sm sm:ml-auto">View public page<ArrowUpRight class="size-4" /></a>
		</div>
		<p class="mono mt-2 text-fg-subtle">{project.uri} · revision {project.revision}</p>
	</div>

	{#if data.created}<FormMessage form={{ success: "Published to the root account's repo." }} />{/if}
	<FormMessage {form} />

	<div class="grid grid-cols-[minmax(0,1fr)] items-start gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
		<div class="min-w-0">
			{#if data.viewer?.isAdmin}
				<ProjectForm action="?/update" values={form?.values ?? data.values} sets={data.sets} avatarUrl={project.avatarUrl} submitLabel="Save and publish" geocoder={data.geocoder} />
			{:else}
				<p class="notice notice-info">Moderators can hide or unhide projects. Only admins edit them directly.</p>
			{/if}
		</div>
		<aside class="flex flex-col gap-6">
			<form method="POST" action="?/status" class="card flex flex-col gap-3 p-5" use:enhance>
				<h2 class="section-title">Visibility</h2>
				<select name="status" class="input" aria-label="Visibility">
					<option value="active" selected={project.status === 'active'}>Active</option>
					<option value="hidden" selected={project.status === 'hidden'}>Hidden</option>
					{#if data.viewer?.isAdmin}<option value="archived" selected={project.status === 'archived'}>Archived</option>{/if}
				</select>
				<input name="reason" class="input" placeholder="Reason (audited)" aria-label="Reason" />
				<button class="btn-secondary w-full">Update</button>
			</form>
			{#if data.viewer?.isAdmin}
				<form method="POST" action="?/managers" class="card flex flex-col gap-3 p-5" use:enhance>
					<h2 class="section-title">Managers</h2>
					{#each data.managers as manager (manager.did)}
						<div class="flex min-h-6 items-center justify-between gap-2">
							<label class="flex min-w-0 items-center gap-2.5 text-small"><input type="checkbox" class="checkbox" name="keep" value={manager.did} checked /><Handle account={manager} class="truncate" /></label>
							{#await data.verifications then statuses}
								{@const status = statuses?.[manager.did]}
								{#if status === 'verified'}
									<StatusPill {status} />
								{:else if status}
									<a class="link shrink-0 text-label" href="/admin/verifications?subject={encodeURIComponent(manager.handle ?? manager.did)}">{status === 'outdated' ? 'Re-issue' : 'Verify'}</a>
								{/if}
							{/await}
						</div>
					{:else}
						<p class="text-small text-fg-muted">Unclaimed.</p>
					{/each}
					<input name="add" class="input" placeholder="Add handle or DID" aria-label="Add manager" />
					<button class="btn-secondary w-full">Save managers</button>
				</form>
			{/if}
			<Card as="section" class="text-small">
				<h2 class="section-title">Requests</h2>
				<ul class="mt-1 divide-y divide-line">
					{#each data.requests as request (request.uri)}
						<li class="flex items-center justify-between gap-2 py-2.5"><a class="min-w-0 text-fg no-underline hover:underline" href="/admin/requests/{request.rkey}">{request.kind} · <Handle account={request.requester} /></a><StatusPill status={request.status} /></li>
					{:else}<li class="py-2.5 text-fg-muted">None.</li>{/each}
				</ul>
			</Card>
			<Card as="section" class="text-small">
				<h2 class="section-title">History</h2>
				<ul class="mt-1 divide-y divide-line">
					{#each data.trail as entry (entry.id)}<li class="py-2.5"><a class="text-fg no-underline hover:underline" href="/admin/audit/{entry.id}">{entry.summary}</a> <span class="text-fg-subtle">· {timeAgo(entry.createdAt)}</span></li>{/each}
				</ul>
			</Card>
		</aside>
	</div>
</div>
