<script lang="ts">
	import { enhance } from '$app/forms';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { timeAgo } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const project = $derived(data.project);
</script>

<svelte:head><title>{project.profile.name} · Staff</title></svelte:head>

<a href="/admin/projects" class="link text-sm">← Projects</a>
<div class="mt-3 flex flex-wrap items-center gap-3">
	<h1 class="text-3xl">{project.profile.name}</h1>
	<StatusPill status={project.status} />
	<a href="/projects/{project.slug}" class="btn-ghost btn-sm">View public page</a>
</div>
<p class="mono muted mt-1">{project.uri} · revision {project.revision}</p>
<div class="mt-6 space-y-4">
	{#if data.created}<FormMessage form={{ success: "Published to the root account's repo." }} />{/if}
	<FormMessage {form} />
</div>

<div class="mt-6 grid gap-6 xl:grid-cols-[1fr_20rem]">
	<!-- min-w-0 lets the form's rows shrink to the column, as they do outside a grid. -->
	<div class="min-w-0">
		{#if data.viewer?.isAdmin}
			<ProjectForm action="?/update" values={form?.values ?? data.values} sets={data.sets} avatarUrl={project.avatarUrl} submitLabel="Save and publish" geocoder={data.geocoder} />
		{:else}
			<p class="card card-pad muted text-sm">Moderators can hide or unhide projects. Only admins edit them directly.</p>
		{/if}
	</div>
	<aside class="space-y-6">
		<form method="POST" action="?/status" class="card card-pad space-y-3" use:enhance>
			<h2 class="section-title">Visibility</h2>
			<select name="status" class="input" aria-label="Visibility">
				<option value="active" selected={project.status === 'active'}>Active</option>
				<option value="hidden" selected={project.status === 'hidden'}>Hidden</option>
				{#if data.viewer?.isAdmin}<option value="archived" selected={project.status === 'archived'}>Archived</option>{/if}
			</select>
			<input name="reason" class="input" placeholder="Reason (audited)" />
			<button class="btn-secondary w-full">Update</button>
		</form>
		{#if data.viewer?.isAdmin}
			<form method="POST" action="?/managers" class="card card-pad space-y-3" use:enhance>
				<h2 class="section-title">Managers</h2>
				{#each data.managers as manager (manager.did)}
					<label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" name="keep" value={manager.did} checked /><Handle account={manager} /></label>
				{:else}
					<p class="muted text-sm">Unclaimed.</p>
				{/each}
				<input name="add" class="input" placeholder="Add handle or DID" />
				<button class="btn-secondary w-full">Save managers</button>
			</form>
		{/if}
		<section class="card card-pad text-sm">
			<h2 class="section-title mb-3">Requests</h2>
			<ul class="space-y-2">
				{#each data.requests as request (request.uri)}
					<li class="flex items-center justify-between gap-2"><a class="hover:underline" href="/admin/requests/{request.rkey}">{request.kind} · <Handle account={request.requester} /></a><StatusPill status={request.status} /></li>
				{:else}<li class="muted">None.</li>{/each}
			</ul>
		</section>
		<section class="card card-pad text-sm">
			<h2 class="section-title mb-3">History</h2>
			<ul class="space-y-2">
				{#each data.trail as entry (entry.id)}<li><a class="hover:underline" href="/admin/audit/{entry.id}">{entry.summary}</a> <span class="muted">· {timeAgo(entry.createdAt)}</span></li>{/each}
			</ul>
		</section>
	</aside>
</div>
