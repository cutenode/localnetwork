<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ComponentProps } from 'svelte';
	import { accountLayout } from '#lib/testing/account.ts';
	import type { BookingEventView, BookingShowLinks } from '#lib/types.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import Page from './+page.svelte';

	/** A booking thread page, with the shows it links to: the planner show it started from and the one announced from it. */
	const { Story } = defineMeta({
		title: 'Dashboard/Booking thread',
		parameters: { sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/dashboard/bookings/3kbooking') } } } }
	});

	const SPACE = 'at://did:plc:root/space/computer.localnetwork.space.booking/3kbooking';

	const pat = { did: 'did:plc:pat', handle: 'pat.test' };

	const neon = { did: 'did:plc:neon', handle: 'neon.test' };

	const event = (author: typeof pat, kind: BookingEventView['kind'], text: string, extra: Partial<BookingEventView> = {}): BookingEventView => ({ uri: `${SPACE}/${author.did}/${text}`, kind, author, createdAt: '2026-10-01T20:00:00Z', text, ...extra });

	const events = [
		event(pat, 'inquiry', 'Free in December?', { show: { title: 'Closing Night', date: '2026-12-12T23:30:00+01:00', venue: 'Kantine', capacity: 300, budget: { amount: 800, currency: 'EUR' } } }),
		event(neon, 'message', 'Yes, works for us.'),
		event(pat, 'status', '', { status: 'confirmed', text: undefined })
	];

	const linked: BookingShowLinks = { plannerShow: '3kplanned', announcedShow: { did: neon.did, rkey: '3kshow' } };

	const data = (did: string, links: BookingShowLinks): ComponentProps<typeof Page>['data'] => ({
		...accountLayout,
		viewer: { ...accountLayout.viewer, did },
		thread: {
			space: SPACE,
			skey: '3kbooking',
			status: 'confirmed',
			title: 'Closing Night',
			showDate: '2026-12-12T23:30:00+01:00',
			project: { uri: 'at://did:plc:root/computer.localnetwork.project/neon', slug: 'neon-tide', name: 'Neon Tide', href: '/projects/neon-tide' },
			promoter: pat,
			participants: [
				{ ...pat, role: 'promoter' },
				{ ...neon, role: 'manager' }
			],
			lastAuthor: pat.did,
			lastActivityAt: '2026-10-01T20:00:00Z',
			createdAt: '2026-10-01T20:00:00Z'
		},
		events,
		links
	});
</script>

<Story name="Promoter, from their planner and announced" asChild><Page params={{ skey: '3kbooking' }} form={null} data={data(pat.did, linked)} /></Story>
<Story name="Promoter, from their planner and announced, phone" asChild globals={phone}><Page params={{ skey: '3kbooking' }} form={null} data={data(pat.did, linked)} /></Story>
<Story name="Manager, announced" asChild><Page params={{ skey: '3kbooking' }} form={null} data={data(neon.did, { ...linked, plannerShow: null })} /></Story>
<Story name="Manager, announced, phone" asChild globals={phone}><Page params={{ skey: '3kbooking' }} form={null} data={data(neon.did, { ...linked, plannerShow: null })} /></Story>
<Story name="Manager, not announced yet" asChild><Page params={{ skey: '3kbooking' }} form={null} data={data(neon.did, { plannerShow: null, announcedShow: null })} /></Story>
<Story name="Manager, not announced yet, phone" asChild globals={phone}><Page params={{ skey: '3kbooking' }} form={null} data={data(neon.did, { plannerShow: null, announcedShow: null })} /></Story>
