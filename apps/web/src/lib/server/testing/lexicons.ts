// Stand-ins for the other authorities' lexicons ours reference or this app writes, which tests load instead of fetching
// the published ones (see loadReferencedLexicons): what this app writes and reads of them.
import { parseLexiconDoc } from '@atproto/lexicon';
import { REFERENCED_NSIDS, type LexiconSource } from '../atproto/lexicons';

const text = { type: 'string' } as const;

/** Lexicon Community's address: a name, a locality, a region and the required country code, with its limits. */
const ADDRESS = {
	lexicon: 1,
	id: 'community.lexicon.location.address',
	defs: {
		main: {
			type: 'object',
			required: ['country'],
			properties: {
				country: { type: 'string', minLength: 2, maxLength: 10 },
				region: text,
				locality: text,
				name: text
			}
		}
	}
};

/** Lexicon Community's coordinates, which this app writes beside the address so calendar apps can map a show. */
const GEO = {
	lexicon: 1,
	id: 'community.lexicon.location.geo',
	defs: {
		main: {
			type: 'object',
			required: ['latitude', 'longitude'],
			properties: { latitude: text, longitude: text, altitude: text, name: text }
		}
	}
};

/** Lexicon Community's Foursquare place, which other calendar apps' events may hold. */
const FSQ = {
	lexicon: 1,
	id: 'community.lexicon.location.fsq',
	defs: {
		main: {
			type: 'object',
			required: ['fsq_place_id'],
			properties: { fsq_place_id: text, latitude: text, longitude: text, name: text }
		}
	}
};

/** Lexicon Community's H3 cell, which other calendar apps' events may hold. */
const HTHREE = {
	lexicon: 1,
	id: 'community.lexicon.location.hthree',
	defs: { main: { type: 'object', required: ['value'], properties: { value: text, name: text } } }
};

const datetime = { type: 'string', format: 'datetime' } as const;

/** Lexicon Community's calendar event: what a show writes, plus the description other calendar apps add. */
const CALENDAR_EVENT = {
	lexicon: 1,
	id: 'community.lexicon.calendar.event',
	defs: {
		main: {
			type: 'record',
			key: 'tid',
			record: {
				type: 'object',
				required: ['createdAt', 'name'],
				properties: {
					name: text,
					description: text,
					createdAt: datetime,
					startsAt: datetime,
					endsAt: datetime,
					mode: text,
					status: text,
					locations: {
						type: 'array',
						items: {
							type: 'union',
							refs: [
								'#uri',
								'community.lexicon.location.address',
								'community.lexicon.location.fsq',
								'community.lexicon.location.geo',
								'community.lexicon.location.hthree'
							]
						}
					},
					uris: { type: 'array', items: { type: 'ref', ref: '#uri' } }
				}
			}
		},
		uri: { type: 'object', required: ['uri'], properties: { uri: { type: 'string', format: 'uri' }, name: text } }
	}
};

const STAND_INS = new Map([ADDRESS, GEO, FSQ, HTHREE, CALENDAR_EVENT].map((doc) => [doc.id, doc]));

export const standInLexicons: LexiconSource = async (nsid) => {
	const doc = STAND_INS.get(nsid);

	if (!doc) throw new Error(`No stand-in for ${nsid} (referenced: ${REFERENCED_NSIDS.join(', ')})`);

	return parseLexiconDoc(doc);
};
