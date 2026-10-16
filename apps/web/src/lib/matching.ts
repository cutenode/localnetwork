import { formatDistance, type LatLng } from './geo';
import { FORMATS, type FeeTier, labelFor, SET_TYPES } from './taxonomy/options';
import type { ProjectView } from './types';

/** What a promoter is looking for. */
export type MatchCriteria = {
	location?: LatLng;
	radiusKm: number;
	genres: string[];
	budgetTier?: FeeTier;
	capacity?: number;
	setType?: string;
	formats?: string[];
};

/** One reason a project fits a show, or one concern, as the directory reports it. */
export type MatchNote = { code: string; genres?: string[]; distanceM?: number; setType?: string; formats?: string[] };

/** How well a project fits on each factor the directory scores, 0 to 100. */
export type MatchBreakdown = Record<'genre' | 'distance' | 'budget' | 'capacity' | 'setType' | 'format', number>;

export type MatchResult = {
	candidate: ProjectView;
	/** 0 to 100. */
	score: number;
	breakdown: MatchBreakdown;
	reasons: string[];
	warnings: string[];
};

const away = (note: MatchNote) => formatDistance((note.distanceM ?? 0) / 1000);

/** A Map, so a code like `constructor` can't resolve to an `Object.prototype` member. */
const NOTE_TEXT = new Map<string, (note: MatchNote) => string>([
	['plays', (note) => `Plays ${(note.genres ?? []).join(', ')}`],
	['adjacentSound', () => 'Adjacent sound'],
	['noGenreOverlap', () => "Genres don't overlap"],
	['locationUnknown', () => 'Location unknown'],
	['local', () => 'Local'],
	['nearby', (note) => `${away(note)} away`],
	['travels', (note) => `Travels ${away(note)}`],
	['outOfRange', (note) => `${away(note)} away`],
	['fitsBudget', () => 'Fits budget'],
	['aboveBudget', () => 'Usually above this budget'],
	['wellAboveBudget', () => 'Well above this budget'],
	['fitsRoom', () => 'Plays rooms this size'],
	['biggerRoom', () => 'Bigger room than usual'],
	['smallerRoom', () => 'Smaller room than usual'],
	['setType', (note) => `Does ${labelFor(SET_TYPES, note.setType).toLowerCase()} sets`],
	['formats', (note) => `Plays ${(note.formats ?? []).map((value) => labelFor(FORMATS, value)).join(' / ')}`],
	['differentSetup', () => 'Different setup']
]);

/** Turns the directory's match notes into the text shown with each match. Codes this app doesn't know are left out. */
export function noteTexts(notes: MatchNote[]): string[] {
	return notes.flatMap((note) => NOTE_TEXT.get(note.code)?.(note) ?? []);
}
