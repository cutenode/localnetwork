<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { StaffMember } from '#lib/types.ts';
	import StaffTable from './StaffTable.svelte';

	const { Story } = defineMeta({ title: 'Staff/Staff', component: StaffTable, tags: ['autodocs'] });

	const member = (did: string, handle: string | null, role: StaffMember['role'], note: string | null, bootstrap = false): StaffMember => ({
		did,
		handle,
		role,
		grantedBy: bootstrap ? null : 'did:plc:ada',
		note,
		createdAt: bootstrap ? null : '2026-10-12T10:00:00Z',
		bootstrap
	});

	const staff = [
		member('did:plc:ada4bsvzr7kd2w6cq5x3nmhe', 'ada.test', 'admin', 'Configured in ADMIN_DIDS', true),
		member('did:plc:mod7xq2kp4wz9d3c6bnhv5fe', 'mod.test', 'moderator', 'Weekend requests'),
		member('did:plc:pat3hk8sz5qw2n7vdc4xm6jr', null, 'moderator', null)
	];
</script>

<Story name="Staff" args={{ staff, viewer: 'did:plc:ada4bsvzr7kd2w6cq5x3nmhe' }} />

<Story name="Staff on a phone" args={{ staff, viewer: 'did:plc:ada4bsvzr7kd2w6cq5x3nmhe' }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />
