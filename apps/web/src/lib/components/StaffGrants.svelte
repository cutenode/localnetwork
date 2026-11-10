<script lang="ts">
	import { enhance } from '$app/forms';
	import { formFields } from '#lib/form-fields.svelte.ts';
	import type { StaffGrant, StaffMember } from '#lib/types.ts';

	/** What a moderator can manage beyond their role. Admins hold it all, so they get no form. */
	let { member }: { member: StaffMember } = $props();

	const fields = $derived(formFields({ grants: member.grants }));

	const OFFERED: { grant: StaffGrant; label: string }[] = [
		{ grant: 'flags', label: 'Flags' },
		{ grant: 'flagGroups', label: 'Flag groups' }
	];
</script>

<form method="POST" action="?/grants" class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1" use:enhance={() => ({ update }) => update({ reset: false })}>
	<input type="hidden" name="did" value={member.did} />
	{#each OFFERED as { grant, label } (grant)}
		<label class="flex items-center gap-1.5 text-label text-fg-muted"><input type="checkbox" class="checkbox" name="grants" value={grant} bind:group={fields.grants} />{label}</label>
	{/each}
	<button class="btn-secondary btn-sm">Save</button>
</form>
