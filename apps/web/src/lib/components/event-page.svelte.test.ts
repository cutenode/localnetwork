import type { ActionResult } from '$app/forms';
import type { ComponentProps } from 'svelte';
import { page, userEvent } from 'vitest/browser';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { capabilitiesFor } from '#lib/capabilities.ts';
import type { EventPageView } from '#lib/events.ts';
import { EVENT_STATUS } from '#lib/shows.ts';
import EventPage from './EventPage.svelte';

const tresor = {
	space: 'at://did:plc:root/computer.localnetwork.space.venue/3tresor',
	skey: '3tresor',
	name: 'Tresor',
	locality: 'Berlin',
	country: 'DE'
};

const event: EventPageView = {
	name: 'Winter Opening',
	description: 'Doors at eleven.',
	startsAt: '2026-12-05T23:00:00+01:00',
	endsAt: '2026-12-06T06:00:00+01:00',
	tickets: 'https://tickets.example/winter',
	status: EVENT_STATUS.scheduled,
	audience: 'public',
	venue: tresor,
	timeZone: 'Europe/Berlin',
	confirmed: false,
	neverConfirmed: false,
	lineup: [
		{ name: 'Crew', href: null, value: '/act/3crew/crew', start: '23:00', end: '02:00' },
		{ name: '@dj.test', href: '/@dj.test', value: 'dj.test' }
	]
};

const party = { name: 'Night Shift', href: '/party/3lzparty/night-shift' };

/** A few of the time zones the server sends while editing. */
const timeZones = ['UTC', 'Europe/Berlin', 'Europe/London'];

const link = 'https://localnetwork.test/party/3lzparty/3mwinter/winter-opening?t=secret';

/** One of the party's hosts, signed in, on the event's page, with what a test stands in for the app's navigation. */
const host = {
	subject: {
		kind: 'event' as const,
		uri: 'at://did:plc:root/space/party/did:plc:root/computer.localnetwork.event/3mwinter'
	},
	can: capabilitiesFor('party', 'host'),
	role: 'owner' as const,
	url: new URL('https://localnetwork.test/party/3lzparty/3mwinter/winter-opening'),
	viewer: { did: 'did:plc:host' },
	navigate: async () => {},
	guardNavigation: () => {},
	enhance: () => ({ destroy: () => {} }),
	version: 'bafyreiopened',
	party
};

/**
 * Stands in for SvelteKit's enhance on the page's own forms, answering each send that isn't called off with `result`,
 * and keeping what was sent.
 */
function answering(result: ActionResult) {
	const sent: { action: string; body: FormData }[] = [];

	const enhance: NonNullable<ComponentProps<typeof EventPage>['enhance']> = (form, submit) => {
		const send = async (event: SubmitEvent) => {
			event.preventDefault();
			const action = new URL(form.action);
			const body = new FormData(form, event.submitter);
			let cancelled = false;

			const callback = await submit?.({
				action,
				formData: body,
				formElement: form,
				submitter: event.submitter,
				cancel: () => (cancelled = true),
				controller: new AbortController()
			});

			if (cancelled) return;
			sent.push({ action: form.action, body });
			// SAFETY: the answers are what the page's `?/cancel` and `?/resetLink` give (changeEvent): nothing, or a refusal.
			await callback?.({ action, formData: body, formElement: form, update: async () => {}, result: result as never });
		};

		const listener = (event: SubmitEvent) => void send(event);
		form.addEventListener('submit', listener);

		return { destroy: () => form.removeEventListener('submit', listener) };
	};

	return { sent, enhance };
}

const bar = () => page.getByRole('region', { name: 'Edit this page' });

const name = () => page.getByRole('textbox', { name: 'Name (required)' });

/** The editor loads after the page draws, and puts focus on the name once its form is listening for changes. */
async function editorReady() {
	await expect.element(name()).toHaveFocus();
}

beforeEach(() => localStorage.clear());

afterEach(() => vi.restoreAllMocks());

describe('EventPage', () => {
	it('shows a visitor its night, venue, party, standing, tickets, lineup and what it is about', async () => {
		await render(EventPage, { event, party, status: 'listed', version: 'bafyreiopened' });

		await expect.element(page.getByRole('heading', { level: 1, name: 'Winter Opening' })).toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'Night Shift' })).toHaveAttribute('href', party.href);
		await expect.element(page.getByText('Dec 5, 2026')).toBeInTheDocument();
		await expect.element(page.getByText('23:00 to 06:00')).toBeInTheDocument();
		await expect.element(page.getByText('Tresor, Berlin')).toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'Tresor' })).toHaveAttribute('href', '/venue/3tresor/tresor');
		await expect.element(page.getByText('Waiting for the venue')).toBeInTheDocument();
		await expect
			.element(page.getByRole('link', { name: 'Tickets' }))
			.toHaveAttribute('href', 'https://tickets.example/winter');
		await expect.element(page.getByText('Doors at eleven.')).toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: '@dj.test' })).toHaveAttribute('href', '/@dj.test');
		await expect
			.element(page.getByRole('listitem').filter({ hasText: 'Crew' }))
			.toMatchTextContent(/Crew\s*23:00 to 02:00/);
		expect(page.getByRole('link', { name: 'Crew' }).elements()).toHaveLength(0);
		expect(bar().elements()).toHaveLength(0);
		expect(page.getByText('Private link').elements()).toHaveLength(0);
	});

	it('says Confirmed once the venue has, and marks a new date', async () => {
		await render(EventPage, {
			event: { ...event, confirmed: true, status: EVENT_STATUS.rescheduled, lineup: [], description: undefined },
			party,
			status: 'listed',
			version: 'bafyreiopened'
		});

		await expect.element(page.getByText('Confirmed')).toBeInTheDocument();
		await expect.element(page.getByText('New date')).toBeInTheDocument();
		expect(page.getByText('Waiting for the venue').elements()).toHaveLength(0);
		expect(page.getByRole('heading', { level: 2 }).elements()).toHaveLength(0);
	});

	it('tells its people an event whose night ended while it waited for the venue was never confirmed', async () => {
		await render(EventPage, { ...host, event: { ...event, neverConfirmed: true }, status: 'listed' });

		await expect.element(page.getByText('Never confirmed')).toBeInTheDocument();
		expect(page.getByText('Waiting for the venue').elements()).toHaveLength(0);
	});

	it('marks a cancelled event, with no tickets, standing or way to cancel it again', async () => {
		await render(EventPage, { ...host, event: { ...event, status: EVENT_STATUS.cancelled }, status: 'listed' });

		await expect.element(page.getByText('Cancelled')).toBeInTheDocument();
		expect(page.getByRole('link', { name: 'Tickets' }).elements()).toHaveLength(0);
		expect(page.getByText('Waiting for the venue').elements()).toHaveLength(0);
		expect(page.getByRole('button', { name: 'Cancel event' }).elements()).toHaveLength(0);
	});

	it('cancels a published event once its host confirms, over the version the page shows', async () => {
		const server = answering({ type: 'success', status: 200, location: host.url.pathname });
		const confirm = vi.spyOn(window, 'confirm').mockReturnValueOnce(false).mockReturnValueOnce(true);
		await render(EventPage, { ...host, enhance: server.enhance, event, status: 'listed' });

		await page.getByRole('button', { name: 'Cancel event' }).click();
		expect(confirm).toHaveBeenCalledWith('Cancel Winter Opening? It stays listed, marked Cancelled.');
		expect(server.sent).toEqual([]);

		await page.getByRole('button', { name: 'Cancel event' }).click();
		await expect.poll(() => server.sent.map(({ body }) => body.get('cid'))).toEqual(['bafyreiopened']);
		expect(server.sent[0].action).toMatch(/\?\/cancel$/);
		expect(page.getByRole('alert').elements()).toHaveLength(0);
	});

	it("gives a private event's people its link to copy, and resets it once they confirm", async () => {
		const copy = vi
			.spyOn(navigator.clipboard, 'writeText')
			.mockResolvedValueOnce()
			.mockRejectedValueOnce(new Error('denied'));

		vi.spyOn(window, 'confirm').mockReturnValue(true);

		const server = answering({
			type: 'failure',
			status: 403,
			location: host.url.pathname,
			data: { error: "You don't have permission to do that" }
		});

		await render(EventPage, {
			...host,
			enhance: server.enhance,
			event: { ...event, audience: 'private' },
			status: 'private',
			link
		});

		await expect.element(page.getByRole('textbox', { name: 'Private link' })).toHaveValue(link);
		await expect.element(page.getByText('Private', { exact: true })).toBeInTheDocument();
		await page.getByRole('button', { name: 'Copy link' }).click();
		expect(copy).toHaveBeenCalledWith(link);
		await expect.element(page.getByRole('button', { name: 'Copied' })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Copy link' }), { timeout: 3000 }).toBeInTheDocument();
		// Copying can be refused, and then it just doesn't say Copied.
		await page.getByRole('button', { name: 'Copy link' }).click();
		expect(copy).toHaveBeenCalledTimes(2);
		expect(page.getByRole('button', { name: 'Copied' }).elements()).toHaveLength(0);

		await page.getByRole('button', { name: 'Reset link' }).click();
		await expect.element(page.getByRole('alert')).toHaveTextContent("You don't have permission to do that");
		expect(server.sent[0].action).toMatch(/\?\/resetLink$/);
	});

	it('hides what only its people see while viewing as a visitor', async () => {
		await render(EventPage, { ...host, event: { ...event, audience: 'private' }, status: 'private', link });

		await page.getByRole('button', { name: 'View as visitor' }).click();
		expect(page.getByText('Private link').elements()).toHaveLength(0);
		expect(page.getByText('Private', { exact: true }).elements()).toHaveLength(0);
		expect(page.getByRole('button', { name: 'Cancel event' }).elements()).toHaveLength(0);
	});

	it('turns each part into its field in edit mode, the venue before its times', async () => {
		await render(EventPage, { ...host, event, status: 'listed', editing: true });

		await expect.element(name()).toHaveValue('Winter Opening');
		await expect.element(page.getByRole('textbox', { name: 'About' })).toHaveValue('Doors at eleven.');
		await expect.element(page.getByText('Tresor')).toBeInTheDocument();
		await expect.element(page.getByLabelText('Date')).toHaveValue('2026-12-05');
		await expect.element(page.getByLabelText('Starts')).toHaveValue('23:00');
		await expect.element(page.getByLabelText('Ends')).toHaveValue('06:00');
		expect(page.getByLabelText('Time zone').elements()).toHaveLength(0);

		const values = (name: string) =>
			[...document.querySelectorAll<HTMLInputElement>(`input[name=${name}]`)].map((field) => field.value);

		expect(values('performer')).toEqual(['/act/3crew/crew', 'dj.test']);
		expect(values('setStart')).toEqual(['23:00', '']);
		await expect
			.element(page.getByRole('textbox', { name: 'Ticket link' }))
			.toHaveValue('https://tickets.example/winter');
		await expect.element(page.getByRole('radio', { name: /^Public/ })).toBeChecked();

		const order = [...document.querySelectorAll('h2')].map((heading) => heading.textContent);
		expect(order.indexOf('Where')).toBeLessThan(order.indexOf('When'));
	});

	it('puts the times on a time zone of their own once there is no venue, and counts that as a change', async () => {
		await render(EventPage, { ...host, timeZones, event, status: 'listed', editing: true });
		await editorReady();
		expect(page.getByLabelText('Time zone').elements()).toHaveLength(0);

		await page.getByRole('button', { name: 'Change venue' }).click();

		// It starts on the clock the times were on, the venue's, so nothing about them moves.
		await expect.element(page.getByRole('combobox', { name: 'Time zone' })).toHaveValue('Europe/Berlin');
		await expect.element(page.getByLabelText('Date')).toBeEnabled();
		await expect.element(bar()).toMatchTextContent(/Unsaved changes/);
	});

	it('asks how the times move to another clock, and saves again with the answer', async () => {
		const server = answering({ type: 'success', status: 200, location: host.url.pathname });

		const form = {
			error: 'Pick which times to keep',
			reauth: false,
			values: { name: 'Winter Opening', publish: 'on' },
			field: { name: 'zoneChange', row: 0 },
			choice: {
				name: 'zoneChange',
				question:
					'The venue is in Europe/Berlin. Keep 22:00 on its clock (22:00 Berlin), or keep the same moment (23:00 Berlin)?',
				options: [
					{ value: 'clock', label: 'Keep 22:00 on its clock' },
					{ value: 'instant', label: 'Keep the same moment' }
				]
			}
		};

		await render(EventPage, { ...host, enhance: server.enhance, event, status: 'draft', editing: true, form });
		await editorReady();

		await expect
			.element(page.getByText(/^The venue is in Europe\/Berlin\. Keep 22:00 on its clock/))
			.toBeInTheDocument();
		await page.getByRole('button', { name: 'Keep the same moment' }).click();

		await expect.poll(() => server.sent.map(({ body }) => body.get('zoneChange'))).toEqual(['instant']);
		// It was a publish that was asked, so answering publishes.
		expect(server.sent[0].body.get('publish')).toBe('on');
	});

	it('says why a save refused someone on the lineup, under it', async () => {
		const form = {
			error: "There's no account called nobody.test",
			reauth: false,
			field: { name: 'performer', row: 1 }
		};

		await render(EventPage, { ...host, event, status: 'listed', editing: true, form });

		await expect.element(page.getByText("There's no account called nobody.test", { exact: true }).last()).toBeVisible();
		await expect
			.element(page.getByRole('textbox', { name: 'Performer' }).first())
			.toHaveAccessibleDescription("There's no account called nobody.test");
	});

	it('counts a lineup row taken off as a change', async () => {
		await render(EventPage, { ...host, event, status: 'listed', editing: true });
		await editorReady();

		await page.getByRole('button', { name: 'Remove from lineup' }).first().click();

		expect(page.getByRole('textbox', { name: 'Performer' }).elements()).toHaveLength(1);
		await expect.element(bar()).toMatchTextContent(/Unsaved changes/);
		await page.getByRole('button', { name: 'Add to lineup' }).click();
		expect(page.getByRole('textbox', { name: 'Performer' }).elements()).toHaveLength(2);
	});

	it('draws an event being made empty, as a draft that needs a name, a venue and a date', async () => {
		await render(EventPage, {
			...host,
			timeZone: 'Europe/London',
			timeZones,
			status: 'draft',
			editing: true,
			version: ''
		});

		await expect.element(page.getByText("This is a draft. Only the party's people can see it.")).toBeInTheDocument();
		await expect.element(name()).toHaveValue('');
		await expect.element(name()).toHaveAttribute('placeholder', 'Event name');
		await expect.element(bar()).toMatchTextContent(/Needs a name, a venue, and a date/);
		await expect.element(page.getByRole('combobox', { name: 'Venue' })).toBeInTheDocument();
		// Until there's a venue its times are on the zone a new draft starts on, which can be changed.
		await expect.element(page.getByRole('combobox', { name: 'Time zone' })).toHaveValue('Europe/London');
		await expect.element(page.getByLabelText('Date')).toBeEnabled();
		expect(page.getByRole('textbox', { name: 'Performer' }).elements()).toHaveLength(1);
		await expect.element(page.getByRole('button', { name: 'Publish' })).toBeInTheDocument();
	});

	it('names the time zone of an event with no venue yet', async () => {
		await render(EventPage, {
			...host,
			event: { ...event, venue: null, timeZone: 'America/New_York' },
			status: 'draft'
		});

		await expect.element(page.getByText('America/New_York')).toBeInTheDocument();
		expect(page.getByText('Waiting for the venue').elements()).toHaveLength(0);
	});

	it('shows a draft without its standing, and its notice only to its people', async () => {
		await render(EventPage, { ...host, event: { ...event, startsAt: undefined, endsAt: undefined }, status: 'draft' });

		await expect.element(page.getByText("This is a draft. Only the party's people can see it.")).toBeInTheDocument();
		expect(page.getByText('Waiting for the venue').elements()).toHaveLength(0);
		expect(page.getByText('23:00 to 06:00').elements()).toHaveLength(0);
		expect(page.getByRole('button', { name: 'Cancel event' }).elements()).toHaveLength(0);
		await page.getByRole('button', { name: 'View as visitor' }).click();
		expect(page.getByText("This is a draft. Only the party's people can see it.").elements()).toHaveLength(0);
	});

	it('takes typing in the name once the editor is ready', async () => {
		await render(EventPage, { ...host, event, status: 'listed', editing: true });
		await editorReady();

		await userEvent.keyboard(' Late');

		await expect.element(name()).toHaveValue('Winter Opening Late');
		await expect.element(bar()).toMatchTextContent(/Unsaved changes/);
	});
});

describe('Going and Interested', () => {
	/** Someone signed in who isn't one of the party's people. */
	const guest = { ...host, can: [], role: undefined, viewer: { did: 'did:plc:fan' } };

	const rsvp = { going: 12, interested: 1, answer: 'going' as const };

	const going = () => page.getByRole('button', { name: /^Going/ });

	const interested = () => page.getByRole('button', { name: /^Interested/ });

	it("shows each answer with how many gave it, the viewer's pressed, and sends a change or takes it back", async () => {
		const server = answering({ type: 'success', status: 200, location: host.url.pathname });
		await render(EventPage, { ...guest, enhance: server.enhance, event, rsvp, status: 'listed' });

		await expect.element(going()).toHaveAttribute('aria-pressed', 'true');
		await expect.element(going()).toHaveAccessibleName('Going 12 people');
		await expect.element(interested()).toHaveAttribute('aria-pressed', 'false');
		await expect.element(interested()).toHaveAccessibleName('Interested 1 person');

		await interested().click();
		await going().click();
		await expect.poll(() => server.sent.map(({ body }) => body.get('answer'))).toEqual(['interested', '']);
		expect(server.sent[0].action).toMatch(/\?\/rsvp$/);
		expect(page.getByRole('alert').elements()).toHaveLength(0);
	});

	it('shows them without a ticket link when there is none', async () => {
		await render(EventPage, { ...guest, event: { ...event, tickets: undefined }, rsvp, status: 'listed' });

		await expect.element(going()).toBeInTheDocument();
		expect(page.getByRole('link', { name: 'Tickets' }).elements()).toHaveLength(0);
	});

	it('links each to sign-in, back to the event, for someone signed out', async () => {
		await render(EventPage, { ...guest, viewer: null, event, rsvp: { ...rsvp, answer: null }, status: 'listed' });

		const next = encodeURIComponent(`${host.url.pathname}#rsvp`);
		await expect.element(page.getByRole('link', { name: /^Going/ })).toHaveAttribute('href', `/login?next=${next}`);
		await expect
			.element(page.getByRole('link', { name: /^Interested/ }))
			.toHaveAttribute('href', `/login?next=${next}`);
		expect(page.getByRole('button', { name: /^Going/ }).elements()).toHaveLength(0);
	});

	it('asks a session from before the app wrote RSVPs to sign in again, and says why anything else failed', async () => {
		const reauth = answering({
			type: 'failure',
			status: 401,
			location: host.url.pathname,
			data: { error: "Sign in again to say you're going", reauth: true }
		});

		const { unmount } = await render(EventPage, { ...guest, enhance: reauth.enhance, event, rsvp, status: 'listed' });

		await interested().click();
		await expect.element(page.getByText("Sign in again to say you're going.", { exact: false })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Sign in again' })).toBeInTheDocument();
		await unmount();

		const down = answering({
			type: 'failure',
			status: 503,
			location: host.url.pathname,
			data: { error: "Going and Interested aren't available right now. Try again in a minute.", reauth: false }
		});

		await render(EventPage, { ...guest, enhance: down.enhance, event, rsvp, status: 'listed' });

		await interested().click();
		await expect
			.element(page.getByRole('alert'))
			.toHaveTextContent("Going and Interested aren't available right now. Try again in a minute.");
		expect(page.getByRole('button', { name: 'Sign in again' }).elements()).toHaveLength(0);
	});

	it('shows a refusal sent without scripts beside the answers, not as a failed save', async () => {
		await render(EventPage, {
			...host,
			event,
			rsvp,
			status: 'listed',
			form: { error: "Sign in again to say you're going", reauth: true, values: { answer: 'going' } }
		});

		await expect.element(page.getByRole('button', { name: 'Sign in again' })).toBeInTheDocument();
		await expect.element(bar()).not.toMatchTextContent(/Sign in again/);
	});

	it('leaves them out for an event that takes no answers, and while editing', async () => {
		const { unmount } = await render(EventPage, { ...guest, event, rsvp: null, status: 'listed' });

		await expect.element(page.getByRole('link', { name: 'Tickets' })).toBeInTheDocument();
		expect(going().elements()).toHaveLength(0);
		await unmount();

		await render(EventPage, { ...host, event, rsvp, status: 'listed', editing: true });
		await editorReady();
		expect(going().elements()).toHaveLength(0);
	});
});
