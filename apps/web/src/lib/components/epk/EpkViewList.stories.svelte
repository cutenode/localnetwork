<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import BookingDetails from '#lib/components/BookingDetails.svelte';
	import type { EpkViewReceipt } from '#lib/epk.ts';
	import EpkViewList from './EpkViewList.svelte';

	const { Story } = defineMeta({ title: 'Press kits/Views', component: EpkViewList, tags: ['autodocs'] });

	const ROOT = 'did:plc:root';

	const view = (id: string, handle: string | null, rkey: string, title: string, viewedAt: string): EpkViewReceipt => ({
		id,
		viewer: { did: `did:plc:${id}`, handle },
		epk: { uri: `at://${ROOT}/computer.localnetwork.epk/${rkey}`, rkey, title },
		viewedAt
	});

	const full = [
		view('3kv1', 'promoter.test', '3kclubs', 'Clubs', '2026-10-29T22:15:00Z'),
		view('3kv2', 'festival-booker.bsky.social', '3kfest', 'Festivals', '2026-10-28T09:40:00Z'),
		view('3kv3', null, '3kclubs', 'Clubs', '2026-10-21T17:05:00Z')
	];

	const pending = <T>() => new Promise<T>(() => {});

	const failed = <T>() => {
		const promise = Promise.reject<T>(new Error('Could not read views'));
		promise.catch(() => {});

		return promise;
	};

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const list = { slug: 'neon-tide', retentionDays: 90 };
</script>

<Story name="Views" args={{ ...list, views: Promise.resolve(full) }} />

<Story name="Views on a phone" args={{ ...list, views: Promise.resolve(full) }} globals={phone} />

<Story name="No views yet" args={{ ...list, views: Promise.resolve([]) }} />

<Story name="Loading" args={{ ...list, views: pending<EpkViewReceipt[]>() }} />

<Story name="Couldn't load" args={{ ...list, views: failed<EpkViewReceipt[]>() }} />

<Story name="Booking details, outside a press kit" asChild>
	<BookingDetails details={null} claimed kitsHref="/projects/neon-tide/press-kits" />
</Story>

<Story name="Booking details, with access" asChild>
	<BookingDetails details={{ bookingEmail: 'bookings@neon-tide.test', rateNotes: 'Clubs from 400 EUR', techRider: '2x CDJ-3000, DJM-V10' }} claimed />
</Story>

<Story name="Booking details, none on file" asChild>
	<BookingDetails details={{}} claimed />
</Story>
