<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { phone } from '#lib/testing/viewports.ts';
	import { localtoolbox } from '#lib/tools/localtoolbox.ts';
	import ToolboxMenu from './ToolboxMenu.svelte';

	/** localtoolbox's tools as its own toolbar menu lists them, beside a capture of the one picked. Proposed for /tools (LOC-1166). */
	const { Story } = defineMeta({ title: 'Patterns/Toolbox menu', component: ToolboxMenu, tags: ['autodocs'] });

	/** The arrow keys move down the list, and wrap from the last to the first. */
	const arrowsMove = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const tabs = within(canvasElement).getAllByRole('tab');

		await userEvent.click(tabs[0]);
		await userEvent.keyboard('{ArrowDown}');
		await expect(tabs[1]).toHaveAttribute('aria-selected', 'true');
		await expect(tabs[1]).toHaveFocus();
		await userEvent.keyboard('{ArrowUp}{ArrowUp}');
		await expect(tabs.at(-1)).toHaveAttribute('aria-selected', 'true');
	};
</script>

<Story name="Menu" args={{ cues: localtoolbox.cues }} play={arrowsMove} />

<Story name="Phone" args={{ cues: localtoolbox.cues }} globals={phone} />
