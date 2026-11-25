<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { BookingEventView, BookingShowLinks, BookingThreadView, ShowDetails, Viewer } from '#lib/types.ts';
	import { phone, tablet } from '#lib/testing/viewports.ts';
	import Page from './+page.svelte';

	/** A booking: the conversation, with the terms and the status moves beside it, or above it on smaller screens. */
	const { Story } = defineMeta({
		title: 'Patterns/Booking thread',
		parameters: { sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/dashboard/bookings/3k1') } } } }
	});

	const NEON = 'did:plc:neon';

	const PAT = 'did:plc:pat';

	const viewer = (did: string): Viewer => ({ did, handle: null, roles: [], isAdmin: false, isStaff: false, spacesCapable: true });

	const day = (offset: number) => new Date(Date.now() + offset * 86_400_000).toISOString().slice(0, 10);

	const neonTide = { uri: 'at://did:plc:root/computer.localnetwork.project/2', slug: 'neon-tide', name: 'Neon Tide', href: '/projects/neon-tide' };

	const thread = (status: string, lastAuthor = PAT, showDate: string | null = day(8)): BookingThreadView => ({
		space: 'at:space/did:plc:root/computer.localnetwork.space.booking/3k1',
		skey: '3k1',
		status,
		title: 'Rooftop Rave',
		showDate,
		project: null,
		promoter: { did: PAT, handle: 'pat.test' },
		participants: [
			{ did: PAT, handle: 'pat.test', role: 'promoter' },
			{ did: NEON, handle: 'neon.test', role: 'manager' }
		],
		lastAuthor,
		lastActivityAt: '2026-11-06T09:00:00Z',
		createdAt: '2026-11-02T19:02:00Z'
	});

	const show: ShowDetails = { title: 'Rooftop Rave', date: `${day(8)}T22:00:00Z`, venue: 'Dachgarten', capacity: 180, budget: { amount: 700, currency: 'EUR' }, setType: 'peak', setMinutes: 120, genres: ['techno', 'electro'] };

	let events = 0;

	const event = (did: string, kind: BookingEventView['kind'], text: string, extra: Partial<BookingEventView> = {}): BookingEventView => ({
		uri: `at://${did}/event/${(events += 1)}`,
		kind,
		author: { did, handle: did === PAT ? 'pat.test' : 'neon.test' },
		createdAt: `2026-11-0${Math.min(events, 6)}T19:02:00Z`,
		text,
		...extra
	});

	const talk = [event(PAT, 'inquiry', "Hi, we're doing the roof on the 14th. Could you do 2 hours from midnight?", { show }), event(NEON, 'message', "Yes, that works. What's the budget?"), event(NEON, 'status', '', { status: 'negotiating', text: undefined }), event(PAT, 'message', '€700. Can you do 2 hours, or do you need 3 for your set?')];

	const moved = (did: string, status: string, text?: string) => event(did, 'status', '', { status, text });

	const confirmed = [...talk, moved(NEON, 'confirmed')];

	const unlinked: BookingShowLinks = { plannerShow: null, announcedShow: null };

	const data = (did: string, status: string, list = talk, overrides = {}) => ({
		viewer: viewer(did),
		pendingRequests: 0,
		unread: 0,
		waiting: 0,
		needs: [],
		access: Promise.resolve([]),
		sections: Promise.resolve(null),
		projects: Promise.resolve([]),
		thread: thread(status),
		project: neonTide,
		events: list,
		links: unlinked,
		...overrides
	});

	const params = { skey: '3k1' };

	const pressButton =
		(name: string) =>
		async ({ canvasElement }: { canvasElement: HTMLElement }) =>
			[...canvasElement.querySelectorAll('button')].find((button) => button.textContent.trim() === name)?.click();

	const answer = pressButton('Decline');

	/** Started from the promoter's planner show and announced by the act's manager. */
	const linked: BookingShowLinks = { plannerShow: '3kplanned', announcedShow: { did: NEON, rkey: '3kshow' } };

	const MARA = 'did:plc:mara';

	/** A thread with a DJ Find DJs lists, who answers for themself. */
	const withDj = (status: string, lastAuthor = PAT) => ({
		thread: { ...thread(status, lastAuthor), participants: [{ did: PAT, handle: 'pat.test', role: 'promoter' as const }, { did: MARA, handle: 'mara.test', role: 'performer' as const }] },
		project: { uri: `at://${MARA}/computer.localnetwork.profile/self`, slug: null, name: 'Mara', href: '/@mara.test' }
	});

	const mara = { author: { did: MARA, handle: 'mara.test' } };

	const confirmedByDj = [talk[0], event(MARA, 'message', 'Yes, I can do 2 hours from midnight.', mara), moved(PAT, 'confirmed')];
</script>

<Story name="Your turn" asChild>
	<Page {params} form={null} data={data(NEON, 'negotiating')} />
</Story>

<Story name="Your turn on a phone" asChild globals={phone}>
	<Page {params} form={null} data={data(NEON, 'negotiating')} />
</Story>

<Story name="Your turn on a tablet" asChild globals={tablet}>
	<Page {params} form={null} data={data(NEON, 'negotiating')} />
</Story>

<Story name="New inquiry" asChild>
	<Page {params} form={null} data={data(NEON, 'open', talk.slice(0, 1))} />
</Story>

<!-- The promoter reads the act's name, never who behind it wrote. -->
<Story name="Waiting on the act" asChild>
	<Page {params} form={null} data={data(PAT, 'negotiating', talk, { thread: thread('negotiating', PAT) })} />
</Story>

<Story name="Waiting on the promoter" asChild>
	<Page {params} form={null} data={data(NEON, 'negotiating', talk, { thread: thread('negotiating', NEON) })} />
</Story>

<Story name="Asking before a move that settles it" asChild play={answer}>
	<Page {params} form={null} data={data(NEON, 'open')} />
</Story>

<Story name="Asking on a phone" asChild globals={phone} play={answer}>
	<Page {params} form={null} data={data(NEON, 'open')} />
</Story>

<Story name="Confirmed" asChild>
	<Page {params} form={null} data={data(NEON, 'confirmed', confirmed)} />
</Story>

<Story name="Confirmed on a phone" asChild globals={phone}>
	<Page {params} form={null} data={data(NEON, 'confirmed', confirmed)} />
</Story>

<!-- The thread links to the planner show it started from, for the promoter only, and to the show announced from it. -->
<Story name="From a planner show and announced" asChild>
	<Page {params} form={null} data={data(PAT, 'confirmed', confirmed, { links: linked })} />
</Story>

<Story name="From a planner show and announced on a phone" asChild globals={phone}>
	<Page {params} form={null} data={data(PAT, 'confirmed', confirmed, { links: linked })} />
</Story>

<!-- Once it's announced, the manager edits that show and can't announce another. -->
<Story name="Announced, for the manager" asChild>
	<Page {params} form={null} data={data(NEON, 'confirmed', confirmed, { links: { ...linked, plannerShow: null } })} />
</Story>

<Story name="Announced, for the manager on a phone" asChild globals={phone}>
	<Page {params} form={null} data={data(NEON, 'confirmed', confirmed, { links: { ...linked, plannerShow: null } })} />
</Story>

<!-- A listed DJ answers an inquiry to them the way an act's manager does. -->
<Story name="New inquiry to a listed DJ" asChild>
	<Page {params} form={null} data={data(MARA, 'open', talk.slice(0, 1), withDj('open'))} />
</Story>

<!-- Shows belong to projects, so a DJ's own confirmed booking has no announce step. -->
<Story name="Confirmed with a listed DJ" asChild>
	<Page {params} form={null} data={data(MARA, 'confirmed', confirmedByDj, withDj('confirmed'))} />
</Story>

<Story name="Date passed" asChild>
	<Page {params} form={null} data={data(NEON, 'negotiating', talk, { thread: thread('negotiating', PAT, day(-97)) })} />
</Story>

<Story name="Declined" asChild>
	<Page {params} form={null} data={data(PAT, 'declined', [...talk, moved(NEON, 'declined', 'We are away that weekend, sorry.')])} />
</Story>

<Story name="Withdrawn" asChild>
	<Page {params} form={null} data={data(NEON, 'cancelled', [...talk, moved(PAT, 'cancelled')])} />
</Story>

<Story name="Cancelled" asChild>
	<Page {params} form={null} data={data(NEON, 'cancelled', [...confirmed, moved(PAT, 'cancelled', 'The venue fell through.')])} />
</Story>

<!-- Everything but the act's name is here already, and it fills in without moving anything. -->
<Story name="Loading the act" asChild>
	<Page {params} form={null} data={data(NEON, 'confirmed', confirmed, { project: new Promise(() => {}) })} />
</Story>

<Story name="Loading the act on a phone" asChild globals={phone}>
	<Page {params} form={null} data={data(PAT, 'negotiating', talk, { project: new Promise(() => {}) })} />
</Story>

<Story name="Directory down" asChild>
	<Page {params} form={null} data={data(NEON, 'confirmed', confirmed, { project: null })} />
</Story>

<Story name="Directory down for a promoter" asChild>
	<Page {params} form={null} data={data(PAT, 'negotiating', talk, { project: null })} />
</Story>

<Story name="No details, no messages" asChild>
	<Page {params} form={null} data={data(NEON, 'open', [], { thread: thread('open', PAT, null) })} />
</Story>

<Story name="A send that failed" asChild>
	<Page {params} form={{ error: "Couldn't send that. Try again.", values: undefined, reauth: false }} data={data(NEON, 'negotiating')} />
</Story>
