<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import FindDjsSwitch from './FindDjsSwitch.svelte';

	const { Story } = defineMeta({ title: 'Directory/Find DJs switch', component: FindDjsSwitch, tags: ['autodocs'] });
</script>

<Story name="Search" args={{ current: '/projects' }} />

<Story name="Match a show" args={{ current: '/match' }} />
