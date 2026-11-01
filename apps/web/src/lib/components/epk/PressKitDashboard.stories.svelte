<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { ACCESS_PRESETS, type EpkConflictView, type EpkDashboard, type EpkDraftView, type EpkView, epkFormDefaults } from '#lib/epk.ts';
	import DraftStatus from './DraftStatus.svelte';
	import EpkConflicts from './EpkConflicts.svelte';
	import EpkEditor from './EpkEditor.svelte';
	import EpkKitList from './EpkKitList.svelte';

	const { Story } = defineMeta({ title: 'Press kits/Dashboard', component: EpkKitList, tags: ['autodocs'] });

	const ROOT = 'did:plc:root';

	const me = { did: 'did:plc:neon', handle: 'neon.test' };

	const co = { did: 'did:plc:remy', handle: 'remy.test' };

	const kit = (rkey: string, title: string, overrides: Partial<EpkView> = {}): EpkView => ({
		uri: `at://${ROOT}/computer.localnetwork.epk/${rkey}`,
		did: ROOT,
		rkey,
		cid: `bafy${rkey}`,
		space: `at://${ROOT}/space/computer.localnetwork.space.epk/${rkey}`,
		legacy: false,
		record: {
			project: `at://${ROOT}/computer.localnetwork.project/3p`,
			title,
			headline: 'Rolling, hypnotic techno for late rooms',
			access: ACCESS_PRESETS[3].access,
			createdAt: '2026-10-01T00:00:00Z',
			updatedAt: '2026-10-20T00:00:00Z'
		},
		...overrides
	});

	const draft = (rkey: string, author: typeof me, status: EpkDraftView['status'], title?: string): EpkDraftView => ({
		uri: `at://${ROOT}/space/computer.localnetwork.space.backstage/3p/${author.did}/computer.localnetwork.epk.draft/${rkey}`,
		rkey,
		author,
		repo: author.did,
		base: status === 'new' ? null : { uri: `at://${ROOT}/computer.localnetwork.epk/${rkey}`, cid: 'bafyold' },
		changes: title ? { title } : { headline: 'Peak-time techno' },
		previous: {},
		fields: [title ? 'title' : 'headline'],
		status,
		createdAt: '2026-10-21T10:00:00Z',
		updatedAt: '2026-10-22T18:30:00Z'
	});

	const full: EpkDashboard = {
		kits: [
			{ epk: kit('3kclubs', 'Clubs'), drafts: [draft('3kclubs', me, 'behind'), draft('3kclubs', co, 'current')] },
			{ epk: kit('3kfest', 'Festivals'), drafts: [] },
			{ epk: kit('3kold', 'Radio', { did: co.did, legacy: true, uri: `at://${co.did}/computer.localnetwork.epk/3kold` }), drafts: [] }
		],
		drafts: [draft('3knew', me, 'new', 'Weddings'), draft('3kgone', co, 'deleted', 'Bars')]
	};

	/** A promise that never settles, for the moment before data arrives. */
	const pending = <T>() => new Promise<T>(() => {});

	const failed = <T>() => {
		const promise = Promise.reject<T>(new Error('Could not read press kits'));
		promise.catch(() => {});

		return promise;
	};

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const list = { slug: 'neon-tide', viewerDid: me.did };

	const values = {
		...epkFormDefaults,
		title: 'Clubs',
		headline: 'Rolling, hypnotic techno for late rooms',
		bio: 'Neon Tide plays long, patient techno sets that build from dub textures into peak-time pressure.',
		email: 'bookings@neon-tide.test'
	};

	const editor = { values, photoBase: null, editHref: '/dashboard/projects/neon-tide/profile' };

	const behind = {
		...editor,
		draft: { status: 'behind' as const, updatedAt: '2026-10-22T18:30:00Z' },
		others: [{ author: co, updatedAt: '2026-10-23T09:15:00Z' }],
		published: true
	};

	const conflicts: EpkConflictView[] = [
		{ field: 'headline', label: 'Headline', published: 'Peak-time techno for big rooms', previous: 'Rolling, hypnotic techno for late rooms', draft: 'Patient techno for late rooms' },
		{ field: 'content.techRider', label: 'Tech rider', published: '3x CDJ-3000, DJM-V10', previous: '2x CDJ-3000', draft: '2x CDJ-3000, Xone:96' }
	];
</script>

<Story name="Kits and drafts" args={{ ...list, dashboard: Promise.resolve(full) }} />

<Story name="Kits and drafts on a phone" args={{ ...list, dashboard: Promise.resolve(full) }} globals={phone} />

<Story name="No press kits yet" args={{ ...list, dashboard: Promise.resolve({ kits: [], drafts: [] }) }} />

<Story name="Loading" args={{ ...list, dashboard: pending<EpkDashboard>() }} />

<Story name="Loading three press kits" args={{ ...list, dashboard: pending<EpkDashboard>(), count: 3 }} />

<Story name="Loading three press kits on a phone" args={{ ...list, dashboard: pending<EpkDashboard>(), count: 3 }} globals={phone} />

<Story name="Loading, none yet" args={{ ...list, dashboard: pending<EpkDashboard>(), count: 0 }} />

<Story name="Couldn't load" args={{ ...list, dashboard: failed<EpkDashboard>() }} />

<Story name="Draft status badges" asChild>
	<div class="flex flex-wrap gap-2">
		<DraftStatus status="new" />
		<DraftStatus status="current" />
		<DraftStatus status="behind" />
		<DraftStatus status="deleted" />
	</div>
</Story>

<Story name="Editor, behind with a co-manager's draft open" asChild>
	<EpkEditor {...behind} />
</Story>

<Story name="Editor on a phone" asChild globals={phone}>
	<EpkEditor {...behind} />
</Story>

<Story name="Editor, press kit deleted since" asChild>
	<EpkEditor {...editor} draft={{ status: 'deleted', updatedAt: '2026-10-22T18:30:00Z' }} others={[]} published={false} />
</Story>

<Story name="Editor, new press kit" asChild>
	<EpkEditor {...editor} values={epkFormDefaults} draft={null} others={[]} published={false} />
</Story>

<Story name="Editor with conflicts" asChild>
	<EpkEditor {...behind} {conflicts} />
</Story>

<Story name="Conflicts" asChild>
	<EpkConflicts {conflicts} />
</Story>

<Story name="Conflicts on a phone" asChild globals={phone}>
	<EpkConflicts {conflicts} />
</Story>
