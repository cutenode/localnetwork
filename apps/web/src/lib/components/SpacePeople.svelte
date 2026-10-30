<script lang="ts">
	import { enhance } from '$app/forms';
	import Handle from '#lib/components/Handle.svelte';
	import { formatDate } from '#lib/format.ts';
	import type { JoinRequestView, PendingInvite, SpacePerson } from '#lib/types.ts';
	import Tag from '#lib/ui/Tag.svelte';

	type Pending = { invites: PendingInvite[]; requests: JoinRequestView[]; links: number };

	let {
		people,
		pending,
		roles
	}: {
		/** Null when they couldn't be read; a pending one shows placeholder rows until it arrives. */
		people: SpacePerson[] | null | Promise<SpacePerson[] | null>;
		pending: Pending | null | Promise<Pending | null>;
		/** The roles the space's kind has. */
		roles: readonly string[];
	} = $props();
</script>

{#snippet who(person: SpacePerson)}
	<div class="flex flex-wrap items-center gap-2"><Handle account={person} />{#if person.hidden}<Tag variant="mono">hidden</Tag>{/if}</div>
	<p class="font-mono text-label text-fg-subtle [overflow-wrap:anywhere]">{person.did}</p>
	<p class="text-label text-fg-muted">Joined {formatDate(person.createdAt)} by {person.via}</p>
{/snippet}

{#snippet roleForm(person: SpacePerson)}
	<form method="POST" action="?/role" class="flex gap-2" use:enhance>
		<input type="hidden" name="did" value={person.did} />
		<select name="role" class="input w-auto min-w-28 capitalize" aria-label="Role for {person.handle ?? person.did}">
			{#each roles as role (role)}<option value={role} selected={role === person.role}>{role}</option>{/each}
		</select>
		<button class="btn-secondary btn-sm">Save</button>
	</form>
{/snippet}

{#snippet removeForm(person: SpacePerson)}
	<form method="POST" action="?/remove" use:enhance={({ cancel }) => { if (!confirm('Remove them from it?')) cancel(); }}>
		<input type="hidden" name="did" value={person.did} />
		<button class="btn-danger btn-sm">Remove</button>
	</form>
{/snippet}

{#snippet ghost(label: string)}
	<div class="card flex flex-col gap-3 p-5" aria-busy="true">
		{#each [0, 1] as row (row)}<div class="h-5 animate-pulse rounded-md bg-raised" aria-hidden="true"></div>{/each}
		<span class="sr-only">{label}</span>
	</div>
{/snippet}

<div class="flex flex-col gap-6">
	{#await people}
		{@render ghost('Loading people')}
	{:then list}
		{#if !list}
			<div class="card p-5 text-small text-fg-muted">Couldn't load its people. <a class="link" href="?">Try again</a></div>
		{:else if list.length}
			<!-- A table where there's room for every column, and a card per person where there isn't. -->
			<div class="@container">
				<div class="card hidden overflow-hidden @3xl:block">
					<table class="table-base">
						<thead><tr><th>Person</th><th>Role</th><th>Can</th><th><span class="sr-only">Actions</span></th></tr></thead>
						<tbody>
							{#each list as person (person.did)}
								<tr>
									<td>{@render who(person)}</td>
									<td>{@render roleForm(person)}</td>
									<td class="text-label text-fg-muted">{person.capabilities.join(', ') || 'nothing'}</td>
									<td class="text-right">{@render removeForm(person)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
				<ul class="flex flex-col gap-3 @3xl:hidden">
					{#each list as person (person.did)}
						<li class="card flex flex-col gap-3 p-4">
							<div>{@render who(person)}</div>
							<p class="text-label text-fg-muted">Can {person.capabilities.join(', ') || 'nothing'}</p>
							<div class="flex flex-wrap items-center justify-between gap-3">{@render roleForm(person)}{@render removeForm(person)}</div>
						</li>
					{/each}
				</ul>
			</div>
		{:else}
			<div class="card p-5 text-small text-fg-muted">Nobody's in it. Add someone to look after it.</div>
		{/if}
	{/await}

	{#await pending}
		{@render ghost('Loading invites and requests')}
	{:then waiting}
		{#if waiting}
			<div class="card flex flex-col gap-3 p-5">
				<h2 class="text-emphasis font-semibold">Waiting</h2>
				<p class="text-small text-fg-muted">{waiting.links === 1 ? '1 invite link works' : `${waiting.links} invite links work`}.</p>
				{#each waiting.invites as invite (invite.did)}
					<p class="text-small">Invited <Handle account={invite} /> as {invite.role}{#if invite.expiresAt}, until {formatDate(invite.expiresAt)}{/if}</p>
				{/each}
				{#each waiting.requests as request (request.did)}
					<p class="text-small"><Handle account={request} /> asked to join{#if request.message}: “{request.message}”{/if}</p>
				{/each}
				{#if !waiting.invites.length && !waiting.requests.length}<p class="text-small text-fg-muted">No invites or requests to join.</p>{/if}
			</div>
		{:else}
			<div class="card p-5 text-small text-fg-muted">Couldn't load invites and requests. <a class="link" href="?">Try again</a></div>
		{/if}
	{/await}
</div>
