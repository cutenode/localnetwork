import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { inboxRow } from '#lib/testing/inbox.ts';
import InboxList from './InboxList.svelte';

it('opens each row through a form that marks it read, saying which are unread', async () => {
	await render(InboxList, {
		rows: [inboxRow('a'), inboxRow('b', { title: 'Closing Night', summary: 'Neon Tide sent a message', unread: false })]
	});

	await expect
		.element(
			page.getByRole('button', {
				name: 'Rooftop Rave The promoter sent 2 messages and changed the status to Negotiating Unread 2h ago'
			})
		)
		.toHaveAttribute('type', 'submit');
	await expect
		.element(page.getByRole('button', { name: 'Closing Night Neon Tide sent a message 2h ago' }))
		.toBeVisible();

	const forms = page
		.getByRole('listitem')
		.elements()
		.map((listed) => listed.querySelector('form'));

	expect(forms.map((form) => [form?.getAttribute('action'), new FormData(form ?? undefined).get('id')])).toEqual([
		['?/open', 'a'],
		['?/open', 'b']
	]);
});

it('holds the room of rows still loading, with nothing to press', async () => {
	await render(InboxList, { loading: 3 });

	await expect.element(page.getByRole('list')).toHaveAttribute('aria-busy', 'true');
	expect(page.getByRole('listitem').elements()).toHaveLength(3);
	expect(page.getByRole('button').elements()).toHaveLength(0);
});
