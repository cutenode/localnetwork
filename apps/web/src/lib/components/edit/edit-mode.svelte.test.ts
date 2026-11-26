import type { ActionResult } from '$app/forms';
import type { ComponentProps } from 'svelte';
import { page, userEvent } from 'vitest/browser';
import { beforeEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { hotkeys } from '#lib/hotkeys.svelte.ts';
import EditPageHarness from './EditPageHarness.svelte';
import { mergeConflict, readFields, writeFields } from './fields.ts';
import type { EditRefusal } from './types.ts';

type Enhance = NonNullable<ComponentProps<typeof EditPageHarness>['enhance']>;

/**
 * Stands in for the page's server behind SvelteKit's enhance, which hands the editor what was sent and the answer:
 * it keeps each send's form, and answers with what the test queued (an error once there's nothing left). SvelteKit's
 * own parsing of an answer needs a running app, which a component test doesn't have.
 */
function fakeServer() {
	const answers: ActionResult[] = [];
	const sent: { action: string; body: FormData }[] = [];

	const enhance: Enhance = (form, submit) => {
		const send = async (event: SubmitEvent) => {
			event.preventDefault();
			const action = new URL(form.action);
			const body = new FormData(form, event.submitter);
			sent.push({ action: form.action, body });

			const callback = await submit?.({
				action,
				formData: body,
				formElement: form,
				submitter: event.submitter,
				cancel: () => {},
				controller: new AbortController()
			});

			const result = answers.shift() ?? { type: 'error', error: { message: 'Down', status: 500 } };

			// SAFETY: the queued answers are what a page's `?/save` gives (savePage), whose refusals are the EditRefusals
			// the editor's submit asks for.
			await callback?.({ action, formData: body, formElement: form, update: async () => {}, result: result as never });
		};

		const listener = (event: SubmitEvent) => void send(event);
		form.addEventListener('submit', listener);

		return { destroy: () => form.removeEventListener('submit', listener) };
	};

	return { answers, sent, enhance };
}

let server = fakeServer();

const EDIT = '/act/halflight?tab=about&edit';

const VIEW = '/act/halflight?tab=about';

const subject = { kind: 'act' as const, uri: 'at:space/did:plc:root/halflight' };

/** One of an act's people on its page, signed in, as the page's load and the root layout's would give them. */
const owner = {
	subject,
	can: ['page.edit' as const],
	role: 'owner' as const,
	status: 'listed' as const,
	url: new URL(`https://localnetwork.test${VIEW}`),
	viewer: { did: 'did:plc:neon' }
};

const bar = () => page.getByRole('region', { name: 'Edit this page' });

const status = (text: string) => page.getByRole('status').filter({ hasText: text });

/** Queues the page's `?/save` answers, as SvelteKit's action responses would be. */
const answer = (
	...results: ({ type: 'redirect'; location: string } | { type: 'failure'; status: number; data: EditRefusal })[]
) => {
	server.answers.push(
		...results.map((result): ActionResult =>
			result.type === 'redirect' ? { ...result, status: 303 } : { ...result, location: VIEW }
		)
	);

	return server.sent;
};

const sentValues = (index: number) =>
	Object.fromEntries(
		[...server.sent[index].body].map(([name, value]) => [name, value instanceof File ? value.name : value])
	);

/** A page whose navigation flips `editing` the way the page's load would, keeping where it was asked to go. */
async function editPage(props: Partial<Parameters<typeof render<typeof EditPageHarness>>[1]> = {}) {
	const went: string[] = [];
	let leave: ((navigation: { willUnload: boolean; to: { url: URL } | null; cancel: () => void }) => void) | undefined;

	const screen = await render(EditPageHarness, {
		...owner,
		enhance: server.enhance,
		...props,
		navigate: async (url: string) => {
			went.push(url);
			await screen.rerender({ editing: new URL(url, 'https://localnetwork.test').searchParams.has('edit') });
		},
		guardNavigation: (guard) => (leave = guard)
	});

	/** A link inside the site taken while editing (or, with none, a reload); whether the guard let it go. */
	const follow = (href?: string) => {
		let cancelled = false;
		leave?.({
			willUnload: !href,
			to: href ? { url: new URL(href, 'https://localnetwork.test') } : null,
			cancel: () => (cancelled = true)
		});

		return !cancelled;
	};

	return { screen, went, follow };
}

/** The editor loads after the page draws, and puts focus on the name once its form is listening for changes. */
async function editorReady() {
	await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveFocus();
}

/** Types into the name once the editor is ready, so what's typed counts as a change. */
async function typeInName(text: string) {
	await editorReady();
	await userEvent.keyboard(text);
}

beforeEach(() => {
	hotkeys.enabled = true;
	localStorage.clear();
	server = fakeServer();
});

describe('edit mode', () => {
	it('draws only the page for a visitor', async () => {
		await render(EditPageHarness, { status: 'listed' });

		await expect.element(page.getByRole('heading', { name: 'Halflight' })).toBeInTheDocument();
		expect(bar().elements()).toHaveLength(0);
	});

	it('draws the bar for its people, with Edit as a link to the page in edit mode', async () => {
		await render(EditPageHarness, owner);

		await expect.element(bar()).toMatchTextContent(/Halflight Public/);
		const edit = page.getByRole('link', { name: 'Edit' });
		await expect.element(edit).toHaveAttribute('href', EDIT);
		await expect.element(edit).toHaveAttribute('aria-keyshortcuts', 'E');
		await expect.element(page.getByText('Bookings and press kits are in its dashboard.')).toBeInTheDocument();
	});

	it("opens the editor on the first field to type in, past a picture's controls", async () => {
		await editPage({ picture: true });

		await page.getByRole('link', { name: 'Edit' }).click();

		await expect.element(page.getByRole('checkbox', { name: 'Remove' })).toBeInTheDocument();
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveFocus();
	});

	it('opens the editor on the first field, and says what is unsaved once something changes', async () => {
		const { went } = await editPage();

		await page.getByRole('link', { name: 'Edit' }).click();

		expect(went).toEqual([EDIT]);
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveFocus();
		await expect.element(bar()).toMatchTextContent(/Editing Nothing changed yet/);
		await expect.element(page.getByRole('button', { name: 'Save' })).toBeDisabled();
		await expect.element(page.getByRole('link', { name: 'Done' })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: /^Add links/ })).toBeInTheDocument();

		await typeInName(' Duo{Enter}');
		await expect.element(bar()).toMatchTextContent(/Unsaved changes Kept in this browser/);
		// A name is one line, so Enter adds nothing.
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveValue('Halflight Duo');
		await expect.element(page.getByRole('button', { name: 'Save' })).toBeEnabled();
		await expect.element(page.getByRole('link', { name: 'Discard' })).toBeInTheDocument();
	});

	it('leaves Edit to the browser when it is opened in a new tab', async () => {
		const { went } = await editPage();
		let leftToBrowser = false;

		const after = (event: Event) => {
			leftToBrowser = !event.defaultPrevented;
			// The test page itself mustn't go anywhere.
			event.preventDefault();
		};

		document.addEventListener('click', after);
		page
			.getByRole('link', { name: 'Edit' })
			.element()
			.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ctrlKey: true }));
		document.removeEventListener('click', after);

		expect(leftToBrowser).toBe(true);
		expect(went).toEqual([]);
	});

	it('opens with E while keyboard shortcuts are on, and not while they are off', async () => {
		const { went } = await editPage();

		hotkeys.enabled = false;
		await userEvent.keyboard('e');
		expect(went).toEqual([]);
		await expect.element(page.getByRole('link', { name: 'Edit' })).not.toHaveAttribute('aria-keyshortcuts');

		hotkeys.enabled = true;
		await userEvent.keyboard('e');
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveFocus();
	});

	it('says when the editor could not load, and tries again', async () => {
		let fails = true;

		const screen = await render(EditPageHarness, {
			...owner,
			navigate: async () => {
				if (fails) throw new Error('offline');
				await screen.rerender({ editing: true });
			}
		});

		await page.getByRole('link', { name: 'Edit' }).click();
		await expect.element(bar()).toMatchTextContent(/Couldn't open the editor/);

		fails = false;
		await page.getByRole('link', { name: 'Try again' }).click();
		await expect.element(bar()).toMatchTextContent(/Editing/);
	});

	it('saves with Cmd+S over the version it opened on, and goes back to the page with Saved', async () => {
		const sent = answer({ type: 'redirect', location: '/act/halflight' });
		const { went } = await editPage();

		await page.getByRole('link', { name: 'Edit' }).click();
		await typeInName(' Duo');
		await userEvent.keyboard('{Meta>}s{/Meta}');

		await expect.element(status('Saved')).toBeInTheDocument();
		expect(sent).toHaveLength(1);
		expect(sent[0].action).toMatch(/\?\/save$/);
		expect(sentValues(0)).toEqual({
			cid: 'bafyreiopened',
			name: 'Halflight Duo',
			tagline: 'Deep, patient house for small rooms'
		});
		expect(went.at(-1)).toBe('/act/halflight');
		await expect.element(page.getByRole('link', { name: 'Edit' })).toHaveFocus();
	});

	it('marks the field the server refused, with its reason and focus', async () => {
		const tagline = 'Deep, patient house for small rooms, and records';
		answer({
			type: 'failure',
			status: 400,
			data: {
				error: 'Tagline can be at most 40 characters',
				values: { tagline },
				reauth: false,
				field: { name: 'tagline', row: 0 }
			}
		});
		await editPage({ editing: true });

		const field = page.getByRole('textbox', { name: 'Tagline' });
		await editorReady();
		await field.fill(tagline);
		await page.getByRole('button', { name: 'Save' }).click();

		await expect.element(field).toHaveFocus();
		await expect.element(field).toHaveAccessibleDescription('Tagline can be at most 40 characters');
		await expect.element(field).toHaveAttribute('aria-invalid', 'true');
		await expect.element(field).toHaveValue(tagline);
		await expect.element(bar()).toMatchTextContent(/Couldn't save Tagline can be at most 40 characters/);
	});

	it('keeps the changes after a failed save, and asks to sign in again when the session ran out', async () => {
		answer({ type: 'failure', status: 401, data: { error: 'Sign in again', reauth: true } });
		await editPage({ editing: true });

		await typeInName(' Duo');
		await page.getByRole('button', { name: 'Save' }).click();
		await expect.element(bar()).toMatchTextContent(/Sign in again to save Your changes stay in this browser/);
		await expect
			.element(page.getByRole('link', { name: 'Sign in again' }))
			.toHaveAttribute('href', `/login?next=${encodeURIComponent(EDIT)}`);
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveValue('Halflight Duo');
	});

	it('says a save failed for any other reason, and keeps Save', async () => {
		answer();
		await editPage({ editing: true });

		await typeInName(' Duo');
		await page.getByRole('button', { name: 'Save' }).click();
		await expect.element(bar()).toMatchTextContent(/Couldn't save Your changes are still here\. Try again/);
		await expect.element(page.getByRole('button', { name: 'Save' })).toBeEnabled();
	});

	it('takes in what only someone else changed and lets the editor pick where both did', async () => {
		answer(
			{
				type: 'failure',
				status: 409,
				data: {
					error: 'The page changed since you opened it.',
					reauth: false,
					values: {},
					conflict: { version: 'bafyreitheirs', values: { name: 'Halflight Trio', tagline: 'House for small rooms' } }
				}
			},
			{ type: 'redirect', location: '/act/halflight' }
		);
		await editPage({ editing: true });

		await typeInName(' Duo');
		await page.getByRole('button', { name: 'Save' }).click();

		await expect.element(bar()).toMatchTextContent(/Someone else saved Pick what to keep, then save/);
		// Only they changed the tagline, so theirs is in.
		await expect.element(page.getByRole('textbox', { name: 'Tagline' })).toHaveValue('House for small rooms');
		const group = page.getByRole('group', { name: 'Name' });
		await expect.element(group.getByRole('radio', { name: 'Yours Halflight Duo' })).toBeChecked();

		await group.getByRole('radio', { name: 'Theirs Halflight Trio' }).click();
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveValue('Halflight Trio');

		await page.getByRole('button', { name: 'Save' }).click();
		await expect.element(status('Saved')).toBeInTheDocument();
		expect(sentValues(1)).toEqual({
			cid: 'bafyreitheirs',
			name: 'Halflight Trio',
			tagline: 'House for small rooms'
		});
	});

	it('discards back to the page, and Undo brings the changes back', async () => {
		const { went } = await editPage({ editing: true });

		await typeInName(' Duo');
		await page.getByRole('link', { name: 'Discard' }).click();

		expect(went).toEqual([VIEW]);
		await expect.element(status('Changes discarded')).toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'Edit' })).toHaveFocus();
		expect(localStorage.length).toBe(0);

		await page.getByRole('button', { name: 'Undo' }).click();
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveValue('Halflight Duo');
		await expect.element(bar()).toMatchTextContent(/Unsaved changes/);

		await page.getByRole('link', { name: 'Discard' }).click();
		await page.getByRole('button', { name: 'Close' }).click();
		await expect.element(status('Changes discarded')).not.toBeInTheDocument();
	});

	it('goes back to the page with nothing to say when nothing changed', async () => {
		const { went } = await editPage({ editing: true });

		await page.getByRole('link', { name: 'Done' }).click();

		expect(went).toEqual([VIEW]);
		await expect.element(page.getByRole('link', { name: 'Edit' })).toHaveFocus();
		expect(page.getByText('Changes discarded').elements()).toHaveLength(0);
	});

	it('asks before a link leaves changes behind, and stays, leaves or saves first', async () => {
		answer({ type: 'redirect', location: '/act/halflight' });
		const { went, follow } = await editPage({ editing: true });

		// Once the editor is in, with nothing changed, links just go.
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveFocus();
		expect(follow('/shows')).toBe(true);
		await typeInName(' Duo');
		expect(follow('/shows')).toBe(false);

		const dialog = page.getByRole('dialog', { name: 'Leave without saving?' });
		await expect.element(dialog.getByRole('button', { name: 'Keep editing' })).toHaveFocus();
		await dialog.getByRole('button', { name: 'Keep editing' }).click();
		await expect.element(dialog).not.toBeInTheDocument();

		expect(follow('/shows')).toBe(false);
		await userEvent.keyboard('{Escape}');
		await expect.element(dialog).not.toBeInTheDocument();

		// A reload gets the browser's own question, not this one.
		expect(follow()).toBe(false);
		expect(dialog.elements()).toHaveLength(0);

		expect(follow('/shows')).toBe(false);
		await dialog.getByRole('button', { name: 'Save and leave' }).click();
		await expect.poll(() => went).toEqual(['https://localnetwork.test/shows']);
	});

	it('discards on the way out when asked to', async () => {
		const { went, follow } = await editPage({ editing: true });

		await typeInName(' Duo');
		expect(follow('/shows')).toBe(false);
		await page.getByRole('dialog').getByRole('button', { name: 'Discard and leave' }).click();

		await expect.poll(() => went).toEqual(['https://localnetwork.test/shows']);
		expect(localStorage.length).toBe(0);
	});

	it('keeps unsaved changes in this browser through View as visitor and back', async () => {
		await editPage({ editing: true });
		await typeInName(' Duo');
		await expect.poll(() => localStorage.length).toBe(1);
		await page.getByRole('button', { name: 'View as visitor' }).click();

		await expect.element(page.getByRole('heading', { name: 'Halflight' })).toBeInTheDocument();
		expect(page.getByText('Bookings and press kits are in its dashboard.').elements()).toHaveLength(0);
		await expect.element(status('Viewing as a visitor')).toBeInTheDocument();

		await page.getByRole('button', { name: 'Back to editing' }).click();
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveValue('Halflight Duo');
	});

	it("opens the menu with the page's own part, and closes it with Escape", async () => {
		await render(EditPageHarness, {
			...owner,
			links: {
				people: ['Neon', 'Sam', 'Ilse'],
				peopleHref: '/people',
				inboxHref: '/inbox',
				waiting: 2,
				dashboardHref: '/dashboard'
			}
		});

		const more = page.getByRole('button', { name: 'More for this page' });
		await more.click();

		await expect.element(more).toHaveAttribute('aria-expanded', 'true');
		await expect.element(page.getByText('Everyone')).toBeInTheDocument();
		await expect.element(page.getByText('Neon, Sam and 1 more')).toBeInTheDocument();
		await expect
			.element(page.getByRole('link', { name: '2 waiting about this page' }))
			.toHaveAttribute('href', '/inbox');
		expect(page.getByText('Everything here is logged as staff').elements()).toHaveLength(0);

		await userEvent.keyboard('{Escape}');
		await expect.element(more).toHaveAttribute('aria-expanded', 'false');
		await expect.element(more).toHaveFocus();
	});

	it('marks staff on a page that is not theirs, and says their editing is logged', async () => {
		const { went } = await editPage({ role: 'staff', status: 'unlisted' });

		const edit = page.getByRole('link', { name: 'Edit as staff' });
		await expect.element(edit).toHaveAttribute('data-sveltekit-preload-data', 'off');
		await expect.element(bar()).toMatchTextContent(/Halflight Staff/);

		await page.getByRole('button', { name: 'More for this page' }).click();
		await expect.element(page.getByText('Everything here is logged as staff')).toBeInTheDocument();
		await expect
			.element(page.getByRole('link', { name: "This page's audit log" }))
			.toHaveAttribute('href', `/admin/audit?q=${encodeURIComponent(subject.uri)}`);
		expect(page.getByText('Shows to').elements()).toHaveLength(0);

		await page
			.getByRole('region', { name: 'Edit this page' })
			.getByRole('link', { name: 'Edit as staff' })
			.last()
			.click();
		expect(went).toEqual([EDIT]);
		await expect.element(bar()).toMatchTextContent(/Opening this was logged/);
		await typeInName(' Duo');
		await expect.element(bar()).toMatchTextContent(/Logged as staff on save/);
	});

	it('draws a draft with Save draft and Publish, and says what it still needs', async () => {
		answer({ type: 'redirect', location: '/act/halflight' });
		await editPage({ editing: true, status: 'draft', missing: ['a name'] });

		await expect.element(bar()).toMatchTextContent(/Draft Needs a name/);
		await expect.element(page.getByRole('button', { name: 'Save draft' })).toBeDisabled();

		// On a phone the menu holds View as visitor and Done.
		await page.getByRole('button', { name: 'More for this page' }).click();
		await page.getByRole('button', { name: 'View as visitor' }).first().click();
		await page.getByRole('button', { name: 'Back to editing' }).click();
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toBeInTheDocument();

		await page.getByRole('button', { name: 'Publish' }).click();
		await expect.element(status('Published')).toBeInTheDocument();
		expect(sentValues(0)).toMatchObject({ publish: 'on' });
	});

	it('shows a save refused without scripts in the bar and the field from the start', async () => {
		await render(EditPageHarness, {
			...owner,
			editing: true,
			form: { error: 'Give it a name', values: { name: '' }, reauth: false, field: { name: 'name', row: 0 } }
		});

		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveValue('');
		await expect
			.element(page.getByRole('textbox', { name: 'Name (required)' }))
			.toHaveAccessibleDescription('Give it a name');
		await expect.element(bar()).toMatchTextContent(/Couldn't save Give it a name/);
	});

	it('starts asking to sign in again when a save without scripts found the session gone', async () => {
		await render(EditPageHarness, {
			...owner,
			editing: true,
			form: { error: 'Sign in again', values: {}, reauth: true }
		});

		await expect.element(bar()).toMatchTextContent(/Sign in again to save/);
	});

	it('offers to sign in again for a sign-in that ran out before editing', async () => {
		await render(EditPageHarness, { ...owner, signedOut: true });

		await expect.element(bar()).toMatchTextContent(/Your sign-in ran out/);
		await expect.element(page.getByRole('link', { name: 'Sign in to edit' })).toBeInTheDocument();
		expect(page.getByRole('link', { name: 'Edit' }).elements()).toHaveLength(0);
	});

	it('draws no Edit for someone who can reach the menu but not change the page', async () => {
		await render(EditPageHarness, { ...owner, can: ['bookings.answer'] });

		await expect.element(page.getByRole('button', { name: 'View as visitor' })).toBeInTheDocument();
		expect(page.getByRole('link', { name: 'Edit' }).elements()).toHaveLength(0);
	});

	it('goes back to the page from the menu while editing a draft', async () => {
		const { went } = await editPage({ editing: true, status: 'draft', missing: [] });

		await expect.element(bar()).toMatchTextContent(/Draft Only its people see it/);
		await page.getByRole('button', { name: 'More for this page' }).click();
		await page.getByRole('button', { name: 'Done' }).click();

		expect(went).toEqual([VIEW]);
	});

	it('opens once however often Edit is pressed while it opens', async () => {
		const went: string[] = [];
		let arrive = () => {};

		const screen = await render(EditPageHarness, {
			...owner,
			navigate: async (url: string) => {
				went.push(url);
				await new Promise<void>((resolve) => (arrive = resolve));
				await screen.rerender({ editing: true });
			}
		});

		const edit = page.getByRole('link', { name: 'Edit' });
		await edit.click();
		await expect.element(edit).toHaveAttribute('aria-disabled', 'true');
		await userEvent.keyboard('e');
		expect(went).toEqual([EDIT]);

		arrive();
		await expect.element(bar()).toMatchTextContent(/Editing/);
	});

	it("says when the editor's code didn't arrive, and loads it again for the menu", async () => {
		let attempts = 0;

		await render(EditPageHarness, {
			...owner,
			editing: true,
			importEditor: async () => {
				attempts++;

				if (attempts === 1) throw new Error('offline');

				return import('./editor.ts');
			}
		});

		await expect.element(bar()).toMatchTextContent(/Couldn't open the editor/);
		await page.getByRole('button', { name: 'More for this page' }).click();
		await expect.element(page.getByText('Shows to')).toBeInTheDocument();
		await expect.element(bar()).not.toMatchTextContent(/Couldn't open the editor/);
		expect(attempts).toBe(2);
	});

	it("goes by the app's own page and account when the page doesn't say, keeping no draft without one", async () => {
		const { url: _url, viewer: _viewer, ...signedOut } = owner;
		await render(EditPageHarness, { ...signedOut, editing: true });

		await typeInName(' Duo');
		await expect.element(bar()).toMatchTextContent(/Unsaved changes/);
		expect(localStorage.length).toBe(0);
	});

	it('says when someone else saved other fields only, with nothing to pick', async () => {
		answer({
			type: 'failure',
			status: 409,
			data: {
				error: 'The page changed since you opened it.',
				reauth: false,
				values: {},
				conflict: { version: 'bafyreitheirs', values: { name: 'Halflight', tagline: 'House for small rooms' } }
			}
		});
		await editPage({ editing: true });

		await typeInName(' Duo');
		await page.getByRole('button', { name: 'Save' }).click();

		await expect
			.element(
				page.getByText('Someone saved this page after you started. Their other changes are already in.', {
					exact: true
				})
			)
			.toBeInTheDocument();
		expect(page.getByRole('radio').elements()).toHaveLength(0);
		await expect.element(page.getByRole('textbox', { name: 'Tagline' })).toHaveValue('House for small rooms');
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveValue('Halflight Duo');
	});
});

describe('writeFields', () => {
	it('puts values into every kind of control, as typing would, and leaves the rest alone', () => {
		const form = document.createElement('form');
		form.innerHTML = `
			<input name="name" value="Halflight">
			<input name="link" value="a"><input name="link" value="b">
			<input type="checkbox" name="genre" value="house" checked><input type="checkbox" name="genre" value="techno">
			<select name="days" multiple><option value="fri" selected>Fri</option><option value="sat">Sat</option></select>
			<input type="file" name="photo">
			<input placeholder="unnamed">
		`;
		const typed: string[] = [];
		form.addEventListener('input', (event) =>
			typed.push(event.target instanceof Element ? (event.target.getAttribute('name') ?? '') : '')
		);

		writeFields(form, [
			['link', 'x'],
			['link', 'y'],
			['genre', 'techno'],
			['days', 'sat']
		]);

		expect(readFields(form)).toEqual([
			['name', 'Halflight'],
			['link', 'x'],
			['link', 'y'],
			['genre', 'techno'],
			['days', 'sat']
		]);
		expect(typed).toEqual(['link', 'link', 'genre', 'genre', 'days']);

		writeFields(form, [['name', 'Halflight']], true);
		expect(readFields(form)).toEqual([
			['name', 'Halflight'],
			['link', ''],
			['link', '']
		]);
	});
});

describe('mergeConflict', () => {
	it('takes what only they changed, leaves what only you changed, and asks where you both did', () => {
		const base = { name: 'A', tagline: 'B', bio: 'C', place: 'D' };
		const theirs = { name: 'A2', tagline: 'B2', bio: 'C', place: 'D3' };
		const yours = { name: 'A', tagline: 'B3', bio: 'C4', place: 'D3' };

		expect(mergeConflict(base, theirs, yours, { tagline: 'Tagline' })).toEqual({
			take: [
				['name', 'A2'],
				['place', 'D3']
			],
			fields: [{ name: 'tagline', label: 'Tagline', theirs: 'B2', yours: 'B3', keep: 'yours' }]
		});
		expect(mergeConflict({}, { links: 'x' }, { links: 'y' }, {}).fields[0]).toMatchObject({ label: 'links' });
	});
});
