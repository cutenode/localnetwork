<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { IssuedVerification, LiveIdentity } from '#lib/types.ts';
	import VerificationTable from './VerificationTable.svelte';

	const { Story } = defineMeta({ title: 'Staff/Verifications', component: VerificationTable, tags: ['autodocs'] });

	const issued = (subject: string, handle: string, displayName: string, createdAt: string): IssuedVerification => ({
		uri: `at://did:plc:root/app.bsky.graph.verification/${subject.slice(-4)}`,
		cid: 'c',
		rkey: subject.slice(-4),
		subject,
		handle,
		displayName,
		createdAt
	});

	const verifications = [
		issued('did:plc:neon', 'neon.test', 'Neon', '2026-10-20T10:00:00Z'),
		issued('did:plc:harbour', 'harbour.test', 'Harbour Lights', '2026-10-12T10:00:00Z'),
		issued('did:plc:pirate', 'pirate.test', 'Pirate Signal', '2026-09-30T10:00:00Z')
	];

	const live = {
		'did:plc:neon': { handle: 'neon.test', displayName: 'Neon' },
		// Renamed since, so Bluesky stopped showing the badge until it's re-issued.
		'did:plc:harbour': { handle: 'harbour.test', displayName: 'HARBOUR LIGHTS' },
		'did:plc:pirate': { handle: null, displayName: 'Pirate Signal' }
	} satisfies Record<string, LiveIdentity>;

	const checking = Object.fromEntries(verifications.map((item) => [item.subject, new Promise<LiveIdentity>(() => {})]));
</script>

<Story name="Verified and out of date" args={{ verifications, live }} />

<Story name="Verified and out of date on a phone" args={{ verifications, live }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="Checking" args={{ verifications, live: checking }} />

<Story name="Nobody verified" args={{ verifications: [], live: {} }} />

<Story name="Couldn't load" args={{ verifications: null, live: {} }} />
