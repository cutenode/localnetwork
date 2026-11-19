<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import HowSignInWorks from './HowSignInWorks.svelte';

	const { Story } = defineMeta({ title: 'Patterns/How signing in works', component: HowSignInWorks, tags: ['autodocs'], args: { id: 'how' } });

	/** Opens the dialog, which has no prop of its own. */
	const open = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		canvasElement.querySelector('dialog')?.showModal();
	};
</script>

<Story name="Open" play={open} />

<Story name="Phone, open" play={open} globals={phone} />
