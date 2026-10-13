import { FEE_TIERS, labelFor, PROJECT_KINDS } from './taxonomy/options';
import type { AccountView, ProjectProfile } from './types';

export function shortDid(did: string): string {
	if (did.length <= 24) return did;

	return `${did.slice(0, 14)}…${did.slice(-6)}`;
}

export function accountLabel(account: AccountView | null | undefined): string {
	if (!account) return 'unknown';

	return account.handle ? `@${account.handle}` : shortDid(account.did);
}

const RELATIVE = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
	['year', 365 * 86_400],
	['month', 30 * 86_400],
	['week', 7 * 86_400],
	['day', 86_400],
	['hour', 3600],
	['minute', 60]
];

export function timeAgo(iso: string | null | undefined, now = Date.now()): string {
	if (!iso) return '';
	const seconds = (Date.parse(iso) - now) / 1000;

	for (const [unit, size] of UNITS) {
		if (Math.abs(seconds) >= size) return RELATIVE.format(Math.round(seconds / size), unit);
	}

	return 'just now';
}

export function formatDate(
	iso: string | null | undefined,
	options: Intl.DateTimeFormatOptions = { dateStyle: 'medium' }
): string {
	if (!iso) return '';
	const date = new Date(iso);

	return Number.isNaN(date.getTime()) ? '' : new Intl.DateTimeFormat('en', options).format(date);
}

export function formatDateTime(iso: string | null | undefined): string {
	return formatDate(iso, { dateStyle: 'medium', timeStyle: 'short' });
}

/** Show dates hold the wall-clock time entered for the venue as if it were UTC, so they're shown in UTC. */
export function formatShowDate(iso: string | null | undefined, withTime = false): string {
	const options: Intl.DateTimeFormatOptions = { dateStyle: 'medium', timeZone: 'UTC' };

	if (withTime) options.timeStyle = 'short';

	return formatDate(iso, options);
}

export function feeLabel(profile: ProjectProfile): string {
	const fee = profile.fee;

	if (!fee) return '';
	const tier = labelFor(FEE_TIERS, fee.tier);

	if (fee.minAmount === undefined && fee.maxAmount === undefined) return tier;

	const money = (amount: number) =>
		fee.currency
			? new Intl.NumberFormat('en', { style: 'currency', currency: fee.currency, maximumFractionDigits: 0 }).format(
					amount
				)
			: amount.toLocaleString('en');

	const range =
		fee.minAmount !== undefined && fee.maxAmount !== undefined
			? `${money(fee.minAmount)}–${money(fee.maxAmount)}`
			: fee.minAmount !== undefined
				? `from ${money(fee.minAmount)}`
				: `up to ${money(fee.maxAmount!)}`;

	return `${tier} · ${range}`;
}

export function kindLabel(kind: string): string {
	return labelFor(PROJECT_KINDS, kind);
}

export function placeLabel(profile: ProjectProfile): string {
	const { city, region, country } = profile.location;

	// A city that's its own region (Berlin, Berlin) is named once.
	return [city, region === city ? undefined : region, country].filter(Boolean).join(', ');
}

export function initials(name: string): string {
	const parts = name
		.replace(/[^\p{L}\p{N}\s]/gu, ' ')
		.trim()
		.split(/\s+/)
		.filter(Boolean);

	if (parts.length === 0) return '?';

	return (parts.length === 1 ? parts[0].slice(0, 2) : parts[0][0] + parts[1][0]).toUpperCase();
}

/** A stable hue per string, for avatar fallbacks. */
export function hueFor(value: string): number {
	let hash = 0;

	for (const char of value) hash = (hash * 31 + char.codePointAt(0)!) % 360;

	return hash;
}

export function pluralize(count: number, singular: string, plural = `${singular}s`): string {
	return `${count.toLocaleString('en')} ${count === 1 ? singular : plural}`;
}
