<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import EditBarFrame from './EditBarFrame.svelte';

	/**
	 * Edit mode in each of its states (docs/edit-mode.md), on a small act page: every state at 1280px in the dark
	 * palette and at 390px in the light one.
	 */
	const { Story } = defineMeta({
		title: 'Patterns/Edit mode',
		component: EditBarFrame,
		parameters: {
			layout: 'fullscreen',
			viewport: {
				options: {
					phone: { name: 'Phone', styles: { width: '390px', height: '844px' }, type: 'mobile' },
					desktop: { name: 'Desktop', styles: { width: '1280px', height: '800px' }, type: 'desktop' }
				}
			}
		}
	});

	const desktop = { viewport: { value: 'desktop', isRotated: false } };

	const phone = { viewport: { value: 'phone', isRotated: false } };

	/** Presses `name` and checks the frame is still drawn: the frame's buttons do nothing, so only the press is the test. */
	const press =
		(name: string) =>
		async ({ canvasElement }: { canvasElement: HTMLElement }) => {
			await userEvent.click(within(canvasElement).getByRole('button', { name }));
			await expect(within(canvasElement).getByRole('region', { name: 'Edit this page' })).toBeInTheDocument();
		};

	/** The light palette (packages/tokens/theme.css), which applies under a `light` class on the root. */
	const light = () => {
		document.documentElement.classList.add('light');

		return () => document.documentElement.classList.remove('light');
	};
</script>

<Story name="View" args={{}} globals={desktop} />

<Story name="View, phone, light" args={{}} globals={phone} beforeEach={light} />

<Story name="Opening the editor" args={{ activity: 'opening' }} globals={desktop} />

<Story name="Opening the editor, phone, light" args={{ activity: 'opening' }} globals={phone} beforeEach={light} />

<Story name="Editor failed to load" args={{ activity: 'open-failed' }} globals={desktop} />

<Story name="Editor failed to load, phone, light" args={{ activity: 'open-failed' }} globals={phone} beforeEach={light} />

<Story name="Editing, nothing changed" args={{ phase: 'edit', dirty: false }} globals={desktop} />

<Story name="Editing, nothing changed, phone, light" args={{ phase: 'edit', dirty: false }} globals={phone} beforeEach={light} />

<Story name="Unsaved" args={{ phase: 'edit', dirty: true }} globals={desktop} />

<Story name="Unsaved, phone, light" args={{ phase: 'edit', dirty: true }} globals={phone} beforeEach={light} />

<Story name="Saving" args={{ phase: 'edit', dirty: true, activity: 'saving' }} globals={desktop} />

<Story name="Saving, phone, light" args={{ phase: 'edit', dirty: true, activity: 'saving' }} globals={phone} beforeEach={light} />

<Story name="Saved" args={{ toast: 'Saved' }} globals={desktop} />

<Story name="Saved, phone, light" args={{ toast: 'Saved' }} globals={phone} beforeEach={light} />

<Story name="Refused field" args={{ phase: 'edit', dirty: true, activity: 'failed', refused: 'Give it a name' }} globals={desktop} />

<Story name="Refused field, phone, light" args={{ phase: 'edit', dirty: true, activity: 'failed', refused: 'Give it a name' }} globals={phone} beforeEach={light} />

<Story name="Save failed" args={{ phase: 'edit', dirty: true, activity: 'failed' }} globals={desktop} />

<Story name="Save failed, phone, light" args={{ phase: 'edit', dirty: true, activity: 'failed' }} globals={phone} beforeEach={light} />

<Story
	name="Conflict"
	args={{ phase: 'edit', dirty: true, activity: 'conflict' }}
	globals={desktop}
	play={async ({ canvasElement }) => {
		const theirs = within(canvasElement).getByRole('radio', { name: /^Theirs/ });
		await userEvent.click(theirs);
		await expect(theirs).toBeChecked();
	}}
/>

<Story name="Conflict, phone, light" args={{ phase: 'edit', dirty: true, activity: 'conflict' }} globals={phone} beforeEach={light} />

<Story name="Sign-in ran out on load" args={{ signedOut: true }} globals={desktop} />

<Story name="Sign-in ran out on load, phone, light" args={{ signedOut: true }} globals={phone} beforeEach={light} />

<Story name="Sign-in ran out on save" args={{ phase: 'edit', dirty: true, activity: 'reauth' }} globals={desktop} />

<Story name="Sign-in ran out on save, phone, light" args={{ phase: 'edit', dirty: true, activity: 'reauth' }} globals={phone} beforeEach={light} />

<Story name="Leaving with changes" args={{ phase: 'edit', dirty: true, leaving: true }} globals={desktop} />

<Story name="Leaving with changes, phone, light" args={{ phase: 'edit', dirty: true, leaving: true }} globals={phone} beforeEach={light} />

<Story name="View as visitor" args={{ phase: 'visitor' }} globals={desktop} />

<Story name="View as visitor, phone, light" args={{ phase: 'visitor' }} globals={phone} beforeEach={light} />

<Story name="Discarded" args={{ toast: 'Changes discarded', undo: true }} globals={desktop} play={press('Undo')} />

<Story name="Discarded, phone, light" args={{ toast: 'Changes discarded', undo: true }} globals={phone} beforeEach={light} />

<Story name="Draft" args={{ phase: 'edit', status: 'draft', dirty: true }} globals={desktop} />

<Story name="Draft, phone, light" args={{ phase: 'edit', status: 'draft', dirty: true }} globals={phone} beforeEach={light} />

<Story name="Draft missing a name" args={{ phase: 'edit', status: 'draft', dirty: false, empty: true, missing: ['a name'], refused: 'Give it a name to publish' }} globals={desktop} />

<Story name="Draft missing a name, phone, light" args={{ phase: 'edit', status: 'draft', dirty: false, empty: true, missing: ['a name'], refused: 'Give it a name to publish' }} globals={phone} beforeEach={light} />

<Story name="Published" args={{ status: 'unlisted', toast: 'Published' }} globals={desktop} />

<Story name="Published, phone, light" args={{ status: 'unlisted', toast: 'Published' }} globals={phone} beforeEach={light} />

<Story name="Menu" args={{ menu: true }} globals={desktop} />

<Story name="Menu, phone, light" args={{ menu: true }} globals={phone} beforeEach={light} />

<Story name="Menu while editing a draft" args={{ phase: 'edit', status: 'draft', dirty: true, menu: true }} globals={desktop} />

<Story name="Menu while editing a draft, phone, light" args={{ phase: 'edit', status: 'draft', dirty: true, menu: true }} globals={phone} beforeEach={light} play={press('Discard')} />

<Story name="Staff, view" args={{ role: 'staff', status: 'unlisted', menu: true }} globals={desktop} />

<Story name="Staff, view, phone, light" args={{ role: 'staff', status: 'unlisted', menu: true }} globals={phone} beforeEach={light} play={async ({ canvasElement }) => {
		await userEvent.click(within(canvasElement).getAllByRole('link', { name: 'Edit as staff' })[0]);
		await expect(within(canvasElement).getByText('Everything here is logged as staff')).toBeVisible();
	}} />

<Story name="Staff, editing" args={{ role: 'staff', phase: 'edit', dirty: true }} globals={desktop} />

<Story name="Staff, editing, phone, light" args={{ role: 'staff', phase: 'edit', dirty: true }} globals={phone} beforeEach={light} />

<Story name="Staff who are its people, menu" args={{ role: 'owner-staff', menu: true }} globals={desktop} />

<Story name="Staff who are its people, menu, phone, light" args={{ role: 'owner-staff', menu: true }} globals={phone} beforeEach={light} />
