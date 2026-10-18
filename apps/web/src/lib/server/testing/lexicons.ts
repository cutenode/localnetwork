// Stand-ins for the other authorities' lexicons ours reference, which tests load instead of fetching the published
// ones (see loadReferencedLexicons): what this app writes and reads of them.
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

export const standInLexicons: LexiconSource = async (nsid) => {
	if (nsid !== ADDRESS.id) throw new Error(`No stand-in for ${nsid} (referenced: ${REFERENCED_NSIDS.join(', ')})`);

	return parseLexiconDoc(ADDRESS);
};
