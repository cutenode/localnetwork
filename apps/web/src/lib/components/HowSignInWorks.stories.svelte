<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import HowSignInWorks from './HowSignInWorks.svelte';

	const { Story } = defineMeta({ title: 'Patterns/How signing in works', component: HowSignInWorks, tags: ['autodocs'], args: { id: 'how' } });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	/** Opens the dialog, which has no prop of its own. */
	const open = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		canvasElement.querySelector('dialog')?.showModal();
	};
</script>

<Story name="Open" play={open} />

<Story name="Phone, open" play={open} globals={phone} />
