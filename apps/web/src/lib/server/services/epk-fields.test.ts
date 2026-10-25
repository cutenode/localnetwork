import { describe, expect, it } from 'vitest';
import { ACCESS_PRESETS } from '#lib/epk.ts';
import { applyChanges, changedFields, cleared, fieldsFrom, fieldsOf, sameValue, valueOf } from './epk-fields';

const access = ACCESS_PRESETS[0].access;

describe('press kit fields', () => {
	it('reads an empty value as none, and compares parsed values whatever order a record had their keys in', () => {
		expect(valueOf({ headline: '' }, 'headline')).toBeUndefined();
		expect(valueOf({ content: { quotes: [] } }, 'content.quotes')).toBeUndefined();
		expect(valueOf({ content: { contact: { email: 'a@b.c' } } }, 'content.contact')).toEqual({ email: 'a@b.c' });
		const contact = valueOf(fieldsFrom([['content.contact', { phone: 'b', email: 'a' }]]), 'content.contact');
		expect(sameValue(contact, { email: 'a', phone: 'b' })).toBe(true);
		expect(sameValue(undefined, {})).toBe(true);
		expect(sameValue('', [])).toBe(true);
		expect(sameValue('a', 'b')).toBe(false);
	});

	it('lists the fields a draft changes, cleared ones included, and how each is cleared', () => {
		expect(changedFields({ title: 'A', headline: '', content: { techRider: 'x', photos: [] } })).toEqual([
			'title',
			'headline',
			'content.photos',
			'content.techRider'
		]);
		expect([cleared('mixes'), cleared('content.contact'), cleared('bio')]).toEqual([[], {}, '']);
	});

	it('applies only the changed fields, dropping the cleared ones', () => {
		const published = fieldsOf(
			{ project: 'at://p', title: 'Clubs', headline: 'Old', bio: 'Bio', access, createdAt: '2026-01-01T00:00:00Z' },
			{ bio: 'Long', techRider: 'CDJs', updatedAt: '2026-01-01T00:00:00Z' }
		);

		expect(published).toEqual({
			title: 'Clubs',
			headline: 'Old',
			bio: 'Bio',
			access,
			content: { bio: 'Long', techRider: 'CDJs' }
		});
		expect(applyChanges(published, { headline: 'New', bio: '', content: { techRider: '' } })).toEqual({
			title: 'Clubs',
			headline: 'New',
			access,
			content: { bio: 'Long' }
		});
		expect(fieldsOf({ project: 'at://p', title: 'T', access, createdAt: '2026-01-01T00:00:00Z' }, null)).toEqual({
			title: 'T',
			access
		});
	});
});
