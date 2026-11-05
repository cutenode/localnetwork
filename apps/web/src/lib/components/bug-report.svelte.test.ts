import { page } from 'vitest/browser';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { stubFormAction } from '#lib/testing/form-action.ts';
import BugReportForm from './BugReportForm.svelte';

const github = 'https://github.com/cutenode/localnetwork/issues/new?template=web-bug.yml';

const facts = { page: '/match', browser: 'Chrome 141, macOS' };

afterEach(() => vi.unstubAllGlobals());

describe('BugReportForm', () => {
	it('asks to approve posting before showing the form, with GitHub as the way out', async () => {
		await render(BugReportForm, { step: { kind: 'consent' }, github, facts });

		await expect.element(page.getByRole('heading', { name: 'Approve posting first' })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Approve' })).toBeEnabled();
		await expect.element(page.getByRole('link', { name: 'Use GitHub instead' })).toHaveAttribute('href', github);
		expect(page.getByRole('textbox', { name: 'Title' }).elements()).toHaveLength(0);
	});

	it('takes a title and what happened, and says what else goes with them', async () => {
		const { container } = await render(BugReportForm, { step: { kind: 'form' }, github, facts });

		// The page stays in the address it posts to, so it's still known after a failure.
		expect(container.querySelector('form')?.getAttribute('action')).toBe('?from=%2Fmatch&/send');
		await expect.element(page.getByRole('textbox', { name: 'Title' })).toBeRequired();
		await expect
			.element(page.getByRole('textbox', { name: 'What happened?' }))
			.toHaveAccessibleDescription('Also sent: /match, Chrome 141, macOS');
		await expect.element(page.getByRole('button', { name: 'Post report' })).toBeEnabled();
	});

	it("doesn't mention extras it doesn't have", async () => {
		const { container } = await render(BugReportForm, { step: { kind: 'form' }, github });

		expect(page.getByText('Also sent').elements()).toHaveLength(0);
		expect(container.querySelector('form')?.getAttribute('action')).toBe('?/send');
	});

	it('holds the button while posting', async () => {
		await render(BugReportForm, { step: { kind: 'form' }, github, facts, sending: true });

		await expect.element(page.getByRole('button', { name: 'Posting…' })).toBeDisabled();
	});

	it('keeps what they wrote after a failure, and offers GitHub filled in with it', async () => {
		const filled = `${github}&title=Blank+map`;
		await render(BugReportForm, {
			step: { kind: 'form' },
			github: filled,
			facts,
			values: { title: 'Blank map', body: 'Never loads.' },
			error: "We couldn't post your report."
		});

		await expect.element(page.getByRole('alert')).toHaveTextContent("We couldn't post your report.");
		await expect.element(page.getByRole('link', { name: 'Send it on GitHub' })).toHaveAttribute('href', filled);
		await expect.element(page.getByRole('button', { name: 'Try again' })).toBeEnabled();
		await expect.element(page.getByRole('textbox', { name: 'Title' })).toHaveValue('Blank map');
		await expect.element(page.getByRole('textbox', { name: 'What happened?' })).toHaveValue('Never loads.');
	});

	it('links to the post once it is up, and back to the page it was about', async () => {
		const url = 'https://userinput.app/d/did:plc:abc/3mabc';
		await render(BugReportForm, { step: { kind: 'sent', url }, github, facts });

		await expect
			.element(page.getByRole('status').getByRole('heading', { name: "Thanks, it's posted" }))
			.toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'View your report' })).toHaveAttribute('href', url);
		await expect.element(page.getByRole('link', { name: 'Back to the site' })).toHaveAttribute('href', '/match');
	});

	it('links back home when the report was about no page in particular', async () => {
		await render(BugReportForm, { step: { kind: 'sent', url: 'https://userinput.app/d/did:plc:abc/3mabc' }, github });

		await expect.element(page.getByRole('link', { name: 'Back to the site' })).toHaveAttribute('href', '/');
	});

	it('posts what they wrote, holding the button until the answer comes', async () => {
		const server = stubFormAction();
		await render(BugReportForm, { step: { kind: 'form' }, github, facts });

		await page.getByRole('textbox', { name: 'Title' }).fill('Blank map');
		await page.getByRole('textbox', { name: 'What happened?' }).fill('The map stays grey.');
		await page.getByRole('button', { name: 'Post report' }).click();
		await expect.element(page.getByRole('button', { name: 'Posting…' })).toBeDisabled();
		expect([server.sent[0]?.get('title'), server.sent[0]?.get('body')]).toEqual(['Blank map', 'The map stays grey.']);
		server.answer();

		await expect.element(page.getByRole('button', { name: 'Post report' })).toBeEnabled();
		await expect.element(page.getByRole('textbox', { name: 'What happened?' })).toHaveValue('The map stays grey.');
	});

	it('holds the button once they approve, while their account server opens', async () => {
		// Approving is a full page POST, which would take the test page with it.
		const stay = (event: Event) => event.preventDefault();
		window.addEventListener('submit', stay, { capture: true });
		await render(BugReportForm, { step: { kind: 'consent' }, github, facts });

		await page.getByRole('button', { name: 'Approve' }).click();

		await expect.element(page.getByRole('button', { name: 'Opening…' })).toBeDisabled();
		window.removeEventListener('submit', stay, { capture: true });
	});
});
