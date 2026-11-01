import { describe, expect, it } from 'vitest';
import { decidedNotice } from './requests';

describe('decidedNotice', () => {
	it('reads each decision outcome and ignores anything else', () => {
		expect(decidedNotice('approved')).toBe('Approved and published');
		expect(decidedNotice('listed')).toBe('Approved and listed in Find DJs');
		expect(decidedNotice('rejected')).toBe('Rejected');
		expect(decidedNotice('changesRequested')).toBe('Sent back for changes');
		expect(decidedNotice('toString')).toBeNull();
		expect(decidedNotice(null)).toBeNull();
	});
});
