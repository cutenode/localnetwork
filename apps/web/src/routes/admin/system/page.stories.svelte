<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { ada } from '#lib/testing/requests.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import SystemPage from './+page.svelte';
	import type { PageProps } from './$types';

	/**
	 * The staff System page with a space whose synced records keep failing to project, listed under it, and HappyView's
	 * health numbers in each state they come in.
	 */
	const { Story } = defineMeta({ title: 'Patterns/System' });

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
		happyView: { url: 'https://localnetwork.hpy.at', reachable: true, sync: { at: inMinutes(-30), error: null }, stats: null },
		backups: null,
		config: {
			publicUrl: 'https://localnetwork.computer',
			serviceDid: 'did:web:localnetwork.computer',
			syncNotify: true,
			pollMs: 60_000,
			geocoder: 'none' as const
		}
	};

	type Stats = NonNullable<PageProps['data']['happyView']['stats']>;

	const script = (trigger: string, runs: number, failures: number, p95Ms: number) => ({ trigger, runs, failures, p95Ms });

	const healthy = {
		jetstream: { connected: true, lagMs: 956 },
		deadLetters: 0,
		scripts: {
			enabled: true,
			scripts: [
				script('record:computer.localnetwork.project', 1_204, 0, 12.4),
				script('xrpc.query:computer.localnetwork.directory.searchPerformers', 312, 0, 84.2),
				script('xrpc.query:computer.localnetwork.probe.sql', 17, 0, 71.577),
				script('record:computer.localnetwork.dj.set', 9, 0, 6.1)
			]
		}
	};

	/** The page with HappyView's API key, its reads answering with `answers` on top of healthy ones. */
	const withStats = (answers: { [K in keyof Stats]?: Awaited<Stats[K]> } = {}) => {
		const stats = { ...healthy, ...answers };

		return {
			...data,
			sync: [space('audit'), space('booking')],
			happyView: {
				...data.happyView,
				stats: {
					jetstream: Promise.resolve(stats.jetstream),
					deadLetters: Promise.resolve(stats.deadLetters),
					scripts: Promise.resolve(stats.scripts)
				}
			}
		};
	};

	const pending = new Promise<never>(() => {});

	const loading = {
		...withStats(),
		happyView: { ...data.happyView, stats: { jetstream: pending, deadLetters: pending, scripts: pending } }
	};

	const failing = {
		enabled: true,
		scripts: [script('record:computer.localnetwork.project', 1_204, 37, 2_480), ...healthy.scripts.scripts.slice(1)]
	};
</script>

<Story name="Records stuck in projection" asChild><SystemPage {data} params={{}} form={null} /></Story>

<Story name="Records stuck in projection, on a phone" asChild globals={phone}><SystemPage {data} params={{}} form={null} /></Story>

<Story name="HappyView healthy" asChild><SystemPage data={withStats()} params={{}} form={null} /></Story>

<Story name="HappyView healthy, on a phone" asChild globals={phone}><SystemPage data={withStats()} params={{}} form={null} /></Story>

<Story name="HappyView loading" asChild><SystemPage data={loading} params={{}} form={null} /></Story>

<Story name="Jetstream lagging" asChild><SystemPage data={withStats({ jetstream: { connected: true, lagMs: 95_000 } })} params={{}} form={null} /></Story>

<Story name="Jetstream disconnected" asChild><SystemPage data={withStats({ jetstream: { connected: false, lagMs: null } })} params={{}} form={null} /></Story>

<Story name="Dead letters and failing scripts" asChild><SystemPage data={withStats({ deadLetters: 12, scripts: failing })} params={{}} form={null} /></Story>

<Story name="Script metrics off" asChild><SystemPage data={withStats({ scripts: { enabled: false, scripts: [] } })} params={{}} form={null} /></Story>

<Story name="No scripts ran" asChild><SystemPage data={withStats({ scripts: { enabled: true, scripts: [] } })} params={{}} form={null} /></Story>

<Story name="HappyView numbers unavailable" asChild><SystemPage data={withStats({ jetstream: null, deadLetters: null, scripts: null })} params={{}} form={null} /></Story>

<Story name="HappyView numbers unavailable, on a phone" asChild globals={phone}><SystemPage data={withStats({ jetstream: null, deadLetters: null, scripts: null })} params={{}} form={null} /></Story>

<Story name="No HappyView API key" asChild><SystemPage data={{ ...data, sync: [space('audit')], happyView: { ...data.happyView, sync: null } }} params={{}} form={null} /></Story>
