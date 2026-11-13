<script lang="ts">
	import { enhance } from '$app/forms';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import StaffTable from '#lib/components/StaffTable.svelte';
	import StrayMembers from '#lib/components/StrayMembers.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { StaffRole } from '#lib/types.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const ROLES: Record<StaffRole, { label: string; hint: string }> = {
		moderator: { label: 'Moderator', hint: 'Decides review requests, hides projects, and reads the audit log.' },
		admin: { label: 'Admin', hint: 'Everything, including staff and direct edits.' }
	};

	let role = $state<StaffRole>('moderator');
</script>

<svelte:head><title>Staff · Local Network</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} title="Staff" lede="Roles are records in the private staff space. Staff also join the member lists of the audit, moderation, staff and industry spaces, so they can read them from any atproto client." />
	<FormMessage {form} />
	<div class="grid grid-cols-[minmax(0,1fr)] items-start gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
		<div class="flex flex-col gap-6">
			<StaffTable staff={data.staff} viewer={data.viewer?.did} />
			{#await data.strays then strays}<StrayMembers {strays} />{/await}
		</div>
		<form method="POST" action="?/grant" class="card flex flex-col gap-4 p-5" use:enhance>
			<h2 class="text-emphasis font-semibold">Add staff</h2>
			<div><label class="label" for="subject">Handle or DID</label><input id="subject" name="subject" class="input" required placeholder="alice.bsky.social" /></div>
			<div>
				<label class="label" for="role">Role</label>
				<select id="role" name="role" class="input" aria-describedby="role-hint" bind:value={role}>
					{#each Object.entries(ROLES) as [value, { label }] (value)}<option {value}>{label}</option>{/each}
				</select>
				<p id="role-hint" class="hint">{ROLES[role].hint}</p>
			</div>
			<div><label class="label" for="note">Note</label><input id="note" name="note" class="input" maxlength="300" /></div>
			<button class="btn-primary w-full">Grant role</button>
		</form>
	</div>
</div>
