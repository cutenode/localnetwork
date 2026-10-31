<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { siteNav } from '#lib/nav.ts';
	import type { Viewer } from '#lib/types.ts';
	import SiteHeader from './SiteHeader.svelte';

	const viewer = (handle: string, isStaff: boolean): Viewer => ({ did: `did:plc:${handle}`, handle: `${handle}.test`, roles: isStaff ? ['moderator'] : [], isAdmin: false, isStaff, spacesCapable: true });

	// The header as the layout gets it from siteNav, signed out, signed in, and as staff with requests waiting.
	const signedOut = siteNav(null, 0);

	const member = siteNav(viewer('neon', false), 0);

	const staff = siteNav(viewer('ada', true), 3);

	const { Story } = defineMeta({
		title: 'Primitives/Site header',
		component: SiteHeader,
		tags: ['autodocs'],
		parameters: { layout: 'fullscreen' },
		args: signedOut
	});

	const desktop = { viewport: { value: 'desktop', isRotated: false } };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	/** Opens the account menu, which has no prop of its own. */
	const openAccount = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		canvasElement.querySelector<HTMLButtonElement>('button[aria-label^="Account"]')?.click();
	};
</script>

<Story name="Signed out" args={{ current: '/match' }} globals={desktop} />

<Story name="Signed in" args={{ ...member, current: '/dashboard/bookings' }} globals={desktop} />

<Story name="Signed in, account menu open" args={member} globals={desktop} play={openAccount} />

<Story name="Staff with requests waiting" args={{ ...staff, current: '/admin' }} globals={desktop} play={openAccount} />

<Story name="Phone" globals={phone} />

<Story name="Phone, menu open" args={{ open: true, current: '/projects' }} globals={phone} />

<Story name="Phone, staff with requests waiting" args={staff} globals={phone} />

<Story name="Phone, staff menu open" args={{ ...staff, open: true }} globals={phone} />
