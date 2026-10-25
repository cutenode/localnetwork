<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import SiteHeader from './SiteHeader.svelte';

	const links = [
		{ href: '/projects', label: 'Find DJs', also: ['/match'] },
		{ href: '/shows', label: 'Shows' },
		{ href: '/charts', label: 'Charts', also: ['/artists'] },
		{ href: '/tools', label: 'Tools' }
	];

	const signedIn = [{ href: '/dashboard', label: 'Dashboard' }];

	const member = { handle: '@neon.test', name: 'neon', links: [], signOut: '/logout' };

	const staff = { handle: '@ada.test', name: 'ada', links: [{ href: '/admin', label: 'Staff console', badge: 3 }], alert: '3 requests waiting', signOut: '/logout' };

	const { Story } = defineMeta({
		title: 'Primitives/Site header',
		component: SiteHeader,
		tags: ['autodocs'],
		parameters: { layout: 'fullscreen' },
		args: {
			links,
			actions: [
				{ href: '/for-djs', label: 'For DJs' },
				{ href: '/login', label: 'Sign in' }
			]
		}
	});

	const desktop = { viewport: { value: 'desktop', isRotated: false } };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	/** Opens the account menu, which has no prop of its own. */
	const openAccount = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		canvasElement.querySelector<HTMLButtonElement>('button[aria-label^="Account"]')?.click();
	};
</script>

<Story name="Signed out" args={{ current: '/match' }} globals={desktop} />

<Story name="Signed in" args={{ actions: signedIn, account: member, current: '/dashboard/bookings' }} globals={desktop} />

<Story name="Signed in, account menu open" args={{ actions: signedIn, account: member }} globals={desktop} play={openAccount} />

<Story name="Staff with requests waiting" args={{ actions: signedIn, account: staff, current: '/admin' }} globals={desktop} play={openAccount} />

<Story name="Phone" globals={phone} />

<Story name="Phone, menu open" args={{ open: true, current: '/projects' }} globals={phone} />

<Story name="Phone, staff with requests waiting" args={{ actions: signedIn, account: staff }} globals={phone} />

<Story name="Phone, staff menu open" args={{ open: true, actions: signedIn, account: staff }} globals={phone} />
