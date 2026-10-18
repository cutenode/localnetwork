import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import BugReportForm from './BugReportForm.svelte';

const github = 'https://github.com/cutenode/localnetwork/issues/new?template=web-bug.yml';

const facts = { page: '/match', browser: 'Chrome 141, macOS' };

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
});
