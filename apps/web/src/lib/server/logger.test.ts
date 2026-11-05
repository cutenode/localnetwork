import { describe, expect, it } from 'vitest';
import { createLogger } from './logger';

describe('createLogger', () => {
	it('logs at the level asked for, pretty or as JSON, under the name given or localnetwork', () => {
		const json = createLogger({ level: 'warn', pretty: false });
		const pretty = createLogger({ level: 'debug', pretty: true, name: 'syncer' });

		expect([json.level, json.bindings()]).toEqual(['warn', { name: 'localnetwork' }]);
		expect([pretty.level, pretty.bindings()]).toEqual(['debug', { name: 'syncer' }]);
	});
});
