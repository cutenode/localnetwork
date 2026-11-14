import { beforeAll } from 'vitest';
import { commands } from 'vitest/browser';

declare module 'vitest/browser' {
	interface BrowserCommands {
		/** `stub-players.ts` */
		stubPlayers: () => Promise<void>;
	}
}

beforeAll(() => commands.stubPlayers());
