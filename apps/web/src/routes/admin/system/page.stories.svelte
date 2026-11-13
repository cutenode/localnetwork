<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { ada } from '#lib/testing/requests.ts';
	import SystemPage from './+page.svelte';

	/** The staff System page with a space whose synced records keep failing to project, listed under it. */
	const { Story } = defineMeta({ title: 'Patterns/System' });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const inMinutes = (minutes: number) => new Date(Date.now() + minutes * 60_000).toISOString();

	const space = (name: string) => ({
		space: `at://did:plc:root/space/computer.localnetwork.space.${name}/main`,
		type: `computer.localnetwork.space.${name}`,
		skey: 'main',
		spaceRev: '3mzlqyh5lt22d',
		lastSyncedAt: inMinutes(-2),
		lastError: null,
		registrationExpiresAt: null,
		repos: [{ did: 'did:plc:root', rev: '3mzlqyh5lt22d', lastSyncedAt: inMinutes(-2), lastError: null }],
		stuck: []
	});

	const data = {
		viewer: ada,
		pendingRequests: 0,
		root: { did: 'did:plc:root', handle: 'localnetwork.computer', service: 'https://pds.localnetwork.computer' },
		spaces: [],
		sync: [
			space('audit'),
			{
				...space('booking'),
				lastError: 'directory offline',
				stuck: [
					{
						did: 'did:plc:pat',
						collection: 'computer.localnetwork.booking.inquiry',
						rkey: '3mzlqyh5lt22e',
						attempts: 4,
						nextAttemptAt: inMinutes(8),
						lastError: 'directory offline'
					},
					{
						did: 'did:plc:neon',
						collection: 'computer.localnetwork.booking.message',
						rkey: '3mzlqyh5lt22f',
						attempts: 1,
						nextAttemptAt: inMinutes(-1),
						lastError: 'directory offline'
					}
				]
			}
		],
		audit: { total: 10, pending: 0, failed: 0, unsupported: 0 },
		failed: [],
		happyView: { url: 'https://happyview.localnetwork.computer', reachable: true, sync: { at: inMinutes(-30), error: null } },
		backups: null,
		config: {
			publicUrl: 'https://localnetwork.computer',
			serviceDid: 'did:web:localnetwork.computer',
			syncNotify: true,
			pollMs: 60_000,
			geocoder: 'none' as const
		}
	};
</script>

<Story name="Records stuck in projection" asChild><SystemPage {data} params={{}} form={null} /></Story>

<Story name="Records stuck in projection, on a phone" asChild globals={phone}><SystemPage {data} params={{}} form={null} /></Story>
