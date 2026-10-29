import { z } from 'zod';
import type { NotificationView } from '#lib/types.ts';
import type { Logger } from '../logger';
import { escapeHtml } from '../tools/markdown';
import type { ChannelSender, Recipient, SendOutcome } from './outbox';

export type Email = { to: string; subject: string; text: string; html: string; headers: Record<string, string> };

/** Sends one email. `idempotencyKey` is the same each time the same email is tried. */
export type EmailSender = { send(email: Email, idempotencyKey: string): Promise<SendOutcome> };

const RESEND_URL = 'https://api.resend.com/emails';

/** What Resend answers a send with. */
const resendSent = z.object({ id: z.string() });

/**
 * Sends through Resend's HTTP API (Railway blocks SMTP below its Pro plan). Resend sends one email per idempotency
 * key for a day, so a retry after a dropped connection can't send it twice.
 */
export class ResendEmailSender implements EmailSender {
	constructor(private readonly options: { apiKey: string; from: string; fetch?: typeof fetch }) {}

	async send(email: Email, idempotencyKey: string): Promise<SendOutcome> {
		const { apiKey, from, fetch = globalThis.fetch } = this.options;
		let response: Response;

		try {
			response = await fetch(RESEND_URL, {
				method: 'POST',
				headers: {
					authorization: `Bearer ${apiKey}`,
					'content-type': 'application/json',
					'idempotency-key': idempotencyKey
				},
				body: JSON.stringify({
					from,
					to: [email.to],
					subject: email.subject,
					text: email.text,
					html: email.html,
					headers: email.headers
				}),
				signal: AbortSignal.timeout(15_000)
			});
		} catch (error) {
			return { kind: 'retryable', error: `Resend: ${String(error)}` };
		}

		if (response.ok) {
			const body = resendSent.safeParse(await response.json().catch(() => null));

			return { kind: 'sent', providerId: body.success ? body.data.id : null };
		}

		const error = `Resend: HTTP ${response.status} ${(await response.text().catch(() => '')).slice(0, 300)}`;

		if (response.status === 429 || response.status >= 500) {
			const retryAfter = Number(response.headers.get('retry-after'));

			return { kind: 'retryable', error, retryAfterMs: retryAfter > 0 ? retryAfter * 1000 : undefined };
		}

		// The address is the only thing in the request that comes from someone else.
		if (response.status === 422) return { kind: 'rejected', reason: 'address_rejected', error };

		return { kind: 'failed', error };
	}
}

/** Writes each email to the log instead, for development, where nothing should leave the machine. */
export class LogEmailSender implements EmailSender {
	constructor(private readonly logger: Logger) {}

	async send(email: Email): Promise<SendOutcome> {
		this.logger.info({ subject: email.subject, text: email.text }, 'email not sent (EMAIL_PROVIDER=log)');

		return { kind: 'sent', providerId: null };
	}
}

/** A batch of inbox items as one email, with a link to turn email off. */
export function renderEmail(items: NotificationView[], to: Recipient & { address: string }, publicUrl: string): Email {
	const inbox = `${publicUrl}/dashboard/inbox`;
	const unsubscribe = `${publicUrl}/notifications/unsubscribe?t=${encodeURIComponent(to.unsubscribeToken ?? '')}`;
	const subject = items.length === 1 ? items[0].title : `${items.length} new things in your Local Network inbox`;
	const line = (item: NotificationView) => [item.title, item.body].filter(Boolean).join(': ');

	const text = [
		...items.map((item) => `${line(item)}\n${publicUrl}${item.href}`),
		`Your inbox: ${inbox}`,
		`Turn these emails off: ${unsubscribe}`
	].join('\n\n');

	const link = (href: string, label: string) => `<a href="${escapeHtml(href)}">${escapeHtml(label)}</a>`;

	const html = [
		'<div style="font-family: system-ui, sans-serif; line-height: 1.5">',
		...items.map((item) => `<p>${link(`${publicUrl}${item.href}`, line(item))}</p>`),
		`<p style="color: #666; font-size: 13px">${link(inbox, 'Your inbox')} · ${link(unsubscribe, 'Turn these emails off')}</p>`,
		'</div>'
	].join('\n');

	return { to: to.address, subject, text, html, headers: { 'List-Unsubscribe': `<${unsubscribe}>` } };
}

/** Email as an outbox channel: items wait two minutes for others, so a burst of replies is one email. */
export function emailChannel(sender: EmailSender, publicUrl: string): ChannelSender {
	return {
		windowMs: 2 * 60_000,
		async send(to, items, key) {
			if (!to.address) return { kind: 'rejected', reason: 'email_unconfirmed', error: 'no address' };

			return sender.send(renderEmail(items, { ...to, address: to.address }, publicUrl), key);
		}
	};
}
