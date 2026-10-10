// See https://svelte.dev/docs/kit/types#app.d.ts
import type { App as ServerApp } from '#lib/server/app.ts';
import type { Logger } from '#lib/server/logger.ts';
import type { Viewer } from '#lib/types.ts';

declare global {
	namespace App {
		interface Error {
			status: number;
			message: string;
			requestId?: string;
		}

		interface Locals {
			app: ServerApp;
			viewer: Viewer | null;
			/** The session's OAuth grant lacks scopes the app now requests, so it has to sign in again. */
			reauth?: boolean;
			requestId: string;
			log: Logger;
			sessionToken?: string;
		}

		interface PageData {
			viewer?: Viewer | null;
		}
	}
}

export {};
