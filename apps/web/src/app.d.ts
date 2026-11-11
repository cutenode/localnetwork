// See https://svelte.dev/docs/kit/types#app.d.ts
import type { App as ServerApp } from '#lib/server/app.ts';
import type { PageMeta } from '#lib/meta.ts';
import type { Logger } from '#lib/server/logger.ts';
import type { FlagPreview, Flags, Viewer } from '#lib/types.ts';

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
			/** Every feature flag for this viewer, or for a signed-out visitor. Not set on XRPC requests. */
			flags: Flags;
			/** The flags a staff member is previewing in this browser, already in `flags`. */
			flagPreview?: FlagPreview;
			/** The session's OAuth grant lacks scopes the app now requests, so it has to sign in again. */
			reauth?: boolean;
			requestId: string;
			log: Logger;
			sessionToken?: string;
			/** Tags only crawlers read, which a public page's load sets and the server hook writes into its HTML. */
			crawlerHead?: string;
		}

		interface PageData {
			viewer?: Viewer | null;
			/** Set by every public page's server load, and written into the head by the root layout. */
			meta?: PageMeta;
		}
	}
}

export {};
