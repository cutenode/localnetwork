<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { submit } from '#lib/testing/submits.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import type { StaffMember } from '#lib/types.ts';
	import StaffGrants from './StaffGrants.svelte';

	const { Story } = defineMeta({ title: 'Staff/Staff grants', component: StaffGrants, tags: ['autodocs'] });

	const moderator = (grants: StaffMember['grants']): StaffMember => ({
		did: 'did:plc:mod7xq2kp4wz9d3c6bnhv5fe',
		handle: 'mod.test',
		role: 'moderator',
		grants,
		grantedBy: 'did:plc:ada',
		note: null,
		createdAt: '2026-10-12T10:00:00Z',
		bootstrap: false
	});
</script>

<Story name="None granted" args={{ member: moderator([]) }} />

<Story name="Flags granted" args={{ member: moderator(['flags']) }} />

<Story name="Everything granted" args={{ member: moderator(['flags', 'flagGroups']) }} />

<Story name="Phone" args={{ member: moderator(['flags']) }} globals={phone} />

<Story
	name="Granting flag groups"
	args={{ member: moderator(['flags']) }}
	play={async ({ canvasElement }) => {
		await userEvent.click(within(canvasElement).getByRole('checkbox', { name: 'Flag groups' }));
		await expect(await submit(canvasElement, 'Save', 'did', 'grants')).toEqual({ did: ['did:plc:mod7xq2kp4wz9d3c6bnhv5fe'], grants: ['flags', 'flagGroups'] });
	}}
/>
