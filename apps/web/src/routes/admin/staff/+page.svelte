<script lang="ts">
	import { enhance } from '$app/forms';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import { formatDate } from '#lib/format.ts';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>Staff · Local Network</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} title="Staff" lede="Roles are records in the private staff space. Staff also join the member lists of the audit, moderation, staff and industry spaces, so they can read them from any atproto client." />
	<FormMessage {form} />
	<div class="grid grid-cols-[minmax(0,1fr)] items-start gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
		<!-- Relative, so the visually hidden header stays inside the scrolling box. -->
		<div class="card relative overflow-x-auto">
			<table class="table-base min-w-150">
				<thead><tr><th>Account</th><th>Role</th><th>Granted</th><th><span class="sr-only">Actions</span></th></tr></thead>
				<tbody>
					{#each data.staff as member (member.did)}
						<tr>
							<td>
								<Handle account={member} />
								<p class="mono text-fg-subtle">{member.did}</p>
								{#if member.note}<p class="mt-1 text-label text-fg-muted">{member.note}</p>{/if}
							</td>
							<td class="capitalize">{member.role}</td>
							<td class="whitespace-nowrap text-fg-muted">{#if member.bootstrap}<span class="font-mono text-label">ADMIN_DIDS</span>{:else}{formatDate(member.createdAt)}{/if}</td>
							<td class="text-right">
								{#if !member.bootstrap && member.did !== data.viewer?.did}
									<form method="POST" action="?/revoke" use:enhance={({ cancel }) => { if (!confirm('Remove this role?')) cancel(); }}>
										<input type="hidden" name="did" value={member.did} />
										<button class="btn-danger btn-sm">Remove</button>
									</form>
								{/if}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
		<form method="POST" action="?/grant" class="card flex flex-col gap-4 p-5" use:enhance>
			<h2 class="text-emphasis font-semibold">Add staff</h2>
			<div><label class="label" for="subject">Handle or DID</label><input id="subject" name="subject" class="input" required placeholder="alice.bsky.social" /></div>
			<div>
				<label class="label" for="role">Role</label>
				<select id="role" name="role" class="input"><option value="moderator">Moderator: reviews requests, hides projects, reads the audit log</option><option value="admin">Admin: everything, including staff and direct edits</option></select>
			</div>
			<div><label class="label" for="note">Note</label><input id="note" name="note" class="input" maxlength="300" /></div>
			<button class="btn-primary w-full">Grant role</button>
		</form>
	</div>
</div>
