<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { HeldRecordView } from '#lib/types.ts';
	import HeldRecordList from './HeldRecordList.svelte';

	/** What Local Network wrote for someone whose account's server couldn't, on their data page. */
	const { Story } = defineMeta({ title: 'Dashboard/Your data', component: HeldRecordList, tags: ['autodocs'] });

	const record = (uri: string, overrides: Partial<HeldRecordView> = {}): HeldRecordView => ({
		uri: `at://did:plc:root/space/x/did:plc:root/${uri}`,
		kind: 'booking',
		title: 'Message in Rooftop Rave',
		excerpt: 'Any news on the 5th? We could push set times back an hour if that helps with travel from Leipzig.',
		createdAt: '2026-10-30T20:00:00Z',
		...overrides
	});

	const records = Promise.resolve([
		record('r1', { kind: 'request', title: 'Claim: Neon Tide', excerpt: "I'm the DJ. My Instagram links back to this page.", createdAt: '2026-10-12T09:00:00Z' }),
		record('b1'),
		record('b2', { title: 'Inquiry in Rooftop Rave, a long show name that runs past the edge of a phone', excerpt: 'Free on the 5th?', createdAt: '2026-10-28T18:00:00Z' }),
		record('e1', { kind: 'epkRequest', title: 'Press kit request for Neon Tide', excerpt: 'For a festival in May', createdAt: '2026-10-02T09:30:00Z' }),
		record('d1', { kind: 'epkDraft', title: 'Press kit draft for Neon Tide', excerpt: null, createdAt: '2026-10-01T09:30:00Z' })
	]);

	const failed = Promise.reject(new Error('directory down'));

	failed.catch(() => {});

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Every kind" args={{ records }} />

<Story name="Loading" args={{ records: new Promise(() => {}) }} />

<Story name="Empty" args={{ records: Promise.resolve([]) }} />

<Story name="Error" args={{ records: failed }} />

<Story name="Deleted" args={{ records, form: { success: 'Deleted 3 records.' } }} />

<Story name="Phone" args={{ records }} globals={phone} />

<Story name="Phone loading" args={{ records: new Promise(() => {}) }} globals={phone} />
