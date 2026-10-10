import { CLAIM_RELATIONSHIPS, isOneOf } from '#lib/taxonomy/options.ts';
import type { ClaimEvidence } from '#lib/types.ts';
import { UserError } from '../core';
import { text } from '../http';

/** Parses the claim form, both when filing a claim and when revising one. */
export function parseClaim(form: FormData): ClaimEvidence {
	const relationship = text(form, 'relationship', 32);

	if (!isOneOf(CLAIM_RELATIONSHIPS, relationship)) throw new UserError('Choose how you are connected to the project');

	const links = text(form, 'links', 5000)
		.split(/\s+/)
		.filter(Boolean)
		.map((link) => {
			try {
				return new URL(link.includes('://') ? link : `https://${link}`).toString();
			} catch {
				throw new UserError(`"${link}" is not a valid link`);
			}
		});

	const unique = [...new Set(links)];

	if (unique.length > 10) throw new UserError('Add up to 10 links');

	const claim: ClaimEvidence = { relationship, evidence: text(form, 'evidence', 20_000) };

	if (unique.length) claim.links = unique;

	return claim;
}
