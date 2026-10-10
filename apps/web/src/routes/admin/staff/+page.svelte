<script lang="ts">
	import { enhance } from '$app/forms';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import { formatDate } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>Staff · Local Network</title></svelte:head>

<h1 class="text-3xl">Staff</h1>
<p class="muted mt-1 max-w-2xl">Roles are records in the private staff space. Staff are also added to the member lists of the audit, moderation, staff, and industry spaces so they can read them from any atproto client.</p>
<div class="mt-6"><FormMessage {form} /></div>
<div class="mt-6 grid gap-6 xl:grid-cols-[1fr_20rem]">
	<div class="card overflow-x-auto">
		<table class="table-base">
			<thead><tr><th>Account</th><th>Role</th><th>Granted</th><th></th></tr></thead>
			<tbody>
				{#each data.staff as member (member.did)}
					<tr>
						<td><Handle account={member} /><p class="mono muted">{member.did}</p>{#if member.note}<p class="muted text-xs">{member.note}</p>{/if}</td>
						<td class="capitalize">{member.role}</td>
						<td class="whitespace-nowrap">{member.bootstrap ? 'ADMIN_DIDS' : formatDate(member.createdAt)}</td>
						<td class="text-right">
							{#if !member.bootstrap && member.did !== data.viewer?.did}
								<form method="POST" action="?/revoke" use:enhance={({ cancel }) => { if (!confirm('Remove this role?')) cancel(); }}>
									<input type="hidden" name="did" value={member.did} />
									<button class="btn-ghost btn-sm text-ansi-bright-red">Remove</button>
								</form>
							{/if}
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
	<form method="POST" action="?/grant" class="card card-pad h-fit space-y-4" use:enhance>
		<h2 class="section-title">Add staff</h2>
		<div><label class="label" for="subject">Handle or DID</label><input id="subject" name="subject" class="input" required placeholder="alice.bsky.social" /></div>
		<div>
			<label class="label" for="role">Role</label>
			<select id="role" name="role" class="input"><option value="moderator">Moderator: reviews requests, hides projects, reads the audit log</option><option value="admin">Admin: everything, including staff and direct edits</option></select>
		</div>
		<div><label class="label" for="note">Note</label><input id="note" name="note" class="input" maxlength="300" /></div>
		<button class="btn-accent w-full">Grant role</button>
	</form>
</div>
