import { normalizeGenres } from '#lib/taxonomy/genres.ts';
import { FEE_TIERS, isOneOf, SET_TYPES } from '#lib/taxonomy/options.ts';
import type { ShowDetails } from '#lib/types.ts';
import { UserError } from '../core';
import { limit, optionalCurrency, optionalInt, optionalText, text } from '../http';

/**
 * A show's date and set time as the wall-clock time the promoter entered. Nothing records the
 * venue's time zone, so it's stored as if it were UTC and shown in UTC (`formatShowDate`), which
 * reads back the same on any server or browser. Without a time it's 20:00.
 */
export function showDate(date: string, time = '20:00'): string {
	const value = new Date(`${date}T${time}:00Z`);

	// Date rolls 2026-02-30 over into March instead of refusing it, so it has to read back the same.
	if (Number.isNaN(value.getTime()) || value.toISOString().slice(0, 16) !== `${date}T${time}`)
		throw new UserError('Pick a valid date and time');

	return value.toISOString();
}

/** Parses the inquiry and planner show forms. Each posts only the fields it has. */
export function parseShow(form: FormData): ShowDetails {
	const title = limit('Show name', text(form, 'title', 1200), 120);

	if (!title) throw new UserError('Give the show a name');
	const date = text(form, 'date', 10);
	const venue = limit('Venue', optionalText(form, 'venue', 1200), 120);
	const capacity = optionalInt(form, 'capacity', 1, 1_000_000);
	const budget = optionalInt(form, 'budgetAmount', 0, 10_000_000);
	const currency = optionalCurrency(form, 'currency');

	if (budget !== undefined && !currency) throw new UserError('Add a currency for the budget');
	const budgetTier = text(form, 'budgetTier', 32);
	const setType = text(form, 'setType', 32);
	const setMinutes = optionalInt(form, 'setMinutes', 15, 720);
	const genres = text(form, 'genres');
	const notes = limit('Notes', optionalText(form, 'notes', 20_000), 2000);

	const show: ShowDetails = { title };

	if (date) show.date = showDate(date, text(form, 'time', 5) || undefined);

	if (venue) show.venue = venue;

	if (capacity) show.capacity = capacity;

	if (budget !== undefined && currency) show.budget = { amount: budget, currency };

	if (isOneOf(FEE_TIERS, budgetTier)) show.budgetTier = budgetTier;

	if (isOneOf(SET_TYPES, setType)) show.setType = setType;

	if (setMinutes) show.setMinutes = setMinutes;

	if (genres) show.genres = normalizeGenres(genres.split(','));

	if (notes) show.notes = notes;

	return show;
}
