<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { BookingListThread, ProjectView, ThreadProjects, Viewer } from '#lib/types.ts';
	import Page from './+page.svelte';

	/** Every booking the viewer is in, by whose turn it is. It renders from the site's own data, whatever the directory does. */
	const { Story } = defineMeta({ title: 'Patterns/Bookings' });

	const NEON = 'did:plc:neon';

	const PAT = 'did:plc:pat';

	const viewer = (did: string): Viewer => ({ did, handle: null, roles: [], isAdmin: false, isStaff: false, spacesCapable: true });

	const day = (offset: number) => new Date(Date.now() + offset * 86_400_000).toISOString().slice(0, 10);

	const hoursAgo = (hours: number) => new Date(Date.now() - hours * 3_600_000).toISOString();

	const thread = (skey: string, title: string, status: string, showDate: string | null, lastAuthor: string, overrides: Partial<BookingListThread> = {}): BookingListThread => ({
		space: `at://did:plc:root/computer.localnetwork.space.booking/${skey}`,
		skey,
		status,
		title,
		showDate,
		project: null,
		promoter: { did: PAT, handle: 'pat.test' },
		participants: [
			{ did: PAT, handle: 'pat.test', role: 'promoter' },
			{ did: NEON, handle: 'neon.test', role: 'manager' }
		],
		lastAuthor,
		lastActivityAt: hoursAgo(Number(skey) + 1),
		createdAt: hoursAgo(72),
		withdrawn: false,
		...overrides
	});

	const live = [
		thread('1', 'Rooftop Rave', 'open', day(8), PAT),
		thread('2', 'Basement Sessions', 'negotiating', day(21), PAT),
		thread('3', 'Walkthrough Night', 'open', null, PAT),
		thread('4', 'Summer Terrace', 'negotiating', day(-97), PAT),
		thread('5', 'Closing Night', 'open', day(15), NEON),
		thread('6', 'Late Lounge', 'negotiating', day(51), NEON),
		thread('7', 'Warehouse Weekender', 'confirmed', day(71), NEON)
	];

	const settled = [
		thread('8', 'Garden Party', 'declined', day(29), NEON),
		thread('9', 'Pop Up Night', 'cancelled', day(43), PAT, { withdrawn: true }),
		thread('10', 'Harbour Boat Party', 'cancelled', day(92), NEON),
		thread('11', 'Spring Opening', 'confirmed', day(-209), NEON)
	];

	const threads = [...live, ...settled];

	const act = (slug: string, name: string) => ({ uri: `at://did:plc:root/computer.localnetwork.project/${slug}`, slug, name, href: `/projects/${slug}` });

	/** Every thread with Neon Tide, but for the ones listed, with Saffron Social. */
	const names = (list: BookingListThread[], saffron: string[] = []): ThreadProjects => new Map(list.map(({ space, skey }) => [space, saffron.includes(skey) ? act('saffron-social', 'Saffron Social') : act('neon-tide', 'Neon Tide')]));

	// SAFETY: the empty state only reads the slug.
	const neonTide = { slug: 'neon-tide' } as ProjectView;

	const data = (did: string, list: BookingListThread[], overrides = {}) => ({
		viewer: viewer(did),
		pendingRequests: 0,
		unread: 0,
		waiting: 0,
		needs: [],
		access: Promise.resolve([]),
		sections: Promise.resolve(null),
		projects: Promise.resolve([neonTide]),
		threads: list,
		acts: 1,
		names: names(list),
		filter: null,
		...overrides
	});

	const twoActs = data(NEON, threads, { acts: 2, names: names(threads, ['3', '6']) });

	const promoter = data(PAT, threads, { acts: 2, names: names(threads, ['3', '6']), projects: Promise.resolve([]) });

	const MARA = 'did:plc:mara';

	const mara = { uri: `at://${MARA}/computer.localnetwork.profile/self`, slug: null, name: 'Mara', href: '/@mara.test' };

	/** The live threads again, with a DJ Find DJs lists, who answers for themself. */
	const withDj = live.map((item) => ({ ...item, participants: [item.participants[0], { did: MARA, handle: 'mara.test', role: 'performer' as const }], lastAuthor: item.lastAuthor === NEON ? MARA : item.lastAuthor }));

	const listedDj = data(MARA, withDj, { names: new Map(withDj.map(({ space }) => [space, mara])), projects: Promise.resolve([]) });

	// The promoter's bookings with an act, and with the listed DJ.
	const actAndDj = [...threads.slice(0, 4), ...withDj.slice(4).map((item) => ({ ...item, skey: `dj${item.skey}`, space: `${item.space}-dj` }))];

	const promoterWithDj = data(PAT, actAndDj, { acts: 2, names: new Map(actAndDj.map(({ space, skey }) => [space, skey.startsWith('dj') ? mara : act('neon-tide', 'Neon Tide')])), projects: Promise.resolve([]) });

	const waiting = new Promise<never>(() => {});

	const slow = { names: waiting, acts: 2 };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const openSettled = async ({ canvasElement }: { canvasElement: HTMLElement }) => canvasElement.querySelector('summary')?.click();
</script>

<Story name="Manager" asChild>
	<Page params={{}} form={null} data={twoActs} />
</Story>

<Story name="Manager on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={twoActs} />
</Story>

<!-- A promoter waits on the act by name, and nothing of theirs is new. -->
<Story name="Promoter" asChild>
	<Page params={{}} form={null} data={promoter} />
</Story>

<Story name="Promoter on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={promoter} />
</Story>

<!-- A listed DJ's inquiries sort by turn as an act manager's do. -->
<Story name="Listed DJ" asChild>
	<Page params={{}} form={null} data={listedDj} />
</Story>

<Story name="Listed DJ on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={listedDj} />
</Story>

<!-- A listed DJ's filter goes by their handle. -->
<Story name="Promoter with an act and a listed DJ" asChild>
	<Page params={{}} form={null} data={promoterWithDj} />
</Story>

<Story name="Filtered to a listed DJ" asChild>
	<Page params={{}} form={null} data={{ ...promoterWithDj, filter: '@mara.test' }} />
</Story>

<Story name="Settled, opened" asChild play={openSettled}>
	<Page params={{}} form={null} data={twoActs} />
</Story>

<Story name="Settled, opened, on a phone" asChild globals={phone} play={openSettled}>
	<Page params={{}} form={null} data={twoActs} />
</Story>

<Story name="One act" asChild>
	<Page params={{}} form={null} data={data(NEON, threads)} />
</Story>

<Story name="Only settled" asChild>
	<Page params={{}} form={null} data={data(NEON, settled)} />
</Story>

<!-- Everything but the act names is here already, and they fill in without moving a row. -->
<Story name="Loading act names" asChild>
	<Page params={{}} form={null} data={data(NEON, threads, slow)} />
</Story>

<Story name="Loading act names on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={data(NEON, threads, slow)} />
</Story>

<!-- Only the directory can say which threads a filter keeps, so the rows hold their room. -->
<Story name="Loading a filter" asChild>
	<Page params={{}} form={null} data={data(NEON, threads, { ...slow, filter: 'neon-tide' })} />
</Story>

<Story name="Filtered to one act" asChild>
	<Page params={{}} form={null} data={{ ...twoActs, filter: 'saffron-social' }} />
</Story>

<Story name="Nothing with this act" asChild>
	<Page params={{}} form={null} data={{ ...twoActs, filter: 'moved-on' }} />
</Story>

<Story name="Directory down" asChild>
	<Page params={{}} form={null} data={data(PAT, threads, { names: null })} />
</Story>

<Story name="Directory down on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={data(NEON, threads, { names: null })} />
</Story>

<Story name="Directory down while filtering" asChild>
	<Page params={{}} form={null} data={data(NEON, threads, { names: null, filter: 'neon-tide' })} />
</Story>

<Story name="Empty for a DJ" asChild>
	<Page params={{}} form={null} data={data(NEON, [])} />
</Story>

<Story name="Empty for a listed DJ" asChild>
	<Page params={{}} form={null} data={data(MARA, [], { projects: Promise.resolve([]), sections: Promise.resolve({ performer: true, promoter: false }) })} />
</Story>

<Story name="Empty for a promoter" asChild>
	<Page params={{}} form={null} data={data(PAT, [], { projects: Promise.resolve([]) })} />
</Story>

<Story name="Empty for a promoter on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={data(PAT, [], { projects: Promise.resolve([]) })} />
</Story>

<Story name="Empty while the directory is slow" asChild>
	<Page params={{}} form={null} data={data(NEON, [], { projects: waiting })} />
</Story>
