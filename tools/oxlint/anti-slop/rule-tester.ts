import { RuleTester } from "oxlint/plugins-dev";
import { describe, it } from "vitest";

// Oxlint's RuleTester only finds a test framework's describe and it as globals, and the root's
// vitest.config.ts doesn't turn those on, so each rule's cases get vitest's here.
RuleTester.describe = describe;
RuleTester.it = it;

export const tester = new RuleTester({ languageOptions: { parserOptions: { lang: "ts" } } });
