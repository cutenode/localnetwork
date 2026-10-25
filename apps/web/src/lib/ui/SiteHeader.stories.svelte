<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import SiteHeader from './SiteHeader.svelte';

	const { Story } = defineMeta({
		title: 'Primitives/Site header',
		component: SiteHeader,
		tags: ['autodocs'],
		parameters: { layout: 'fullscreen' },
		args: {
			links: [
				{ href: '/projects', label: 'DJs' },
				{ href: '/mixes', label: 'Mixes' },
				{ href: '/events', label: 'Events' },
				{ href: '/match', label: 'For promoters' },
				{ href: '/tools', label: 'Tools' }
			],
			account: { href: '/login', label: 'Sign in' },
			cta: { href: '/dashboard/projects/new', label: 'List your project' }
		}
	});
</script>

<Story name="Wide" args={{ current: '/mixes' }} />

<Story name="Phone" globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="Phone, menu open" args={{ open: true }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />
