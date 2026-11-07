import { tester } from "../rule-tester.ts";

import { noWidenThenAssertRule } from "./no-widen-then-assert.ts";

const error = { messageId: "widenThenAssert" };

tester.run("anti-slop/no-widen-then-assert", noWidenThenAssertRule, {
	valid: [
		"const source = { id: 'first' }; const widened: unknown = source;",
		"declare const input: unknown; const parsed = input as { readonly id: string };",
	],
	invalid: [
		{
			code: "const source = { id: 'second' }; const widened: unknown = source; const parsed = widened as { readonly id: string };",
			errors: [error],
		},
	],
});

// Local cases, beyond upstream's: each broad kind, the assertion forms, and the flows that keep or
// lose the original type.
tester.run("anti-slop/no-widen-then-assert (local)", noWidenThenAssertRule, {
	valid: [
		"let widened: unknown = 1; const parsed = widened as number;",
		"const widened: unknown = 1; widened = 2; const parsed = widened as number;",
		"const widened: unknown = read(); const parsed = widened as number;",
		"const widened: unknown = 1; const parsed = widened as object;",
		"const widened: string = 'a'; const parsed = widened as 'a';",
		"const { widened }: { widened: unknown } = { widened: 1 }; const parsed = widened as number;",
		"const source = { id: 'a' }; const widened: unknown = source; function f() { return widened as { id: string }; }",
		"function f() { const source = 1; return () => { const widened: unknown = source; return widened as number; }; }",
		"const source = { id: 'a' }; const widened: object = source; const parsed = widened as { id: string } | null;",
		"const source = { id: 'a' }; const widened: object = source; const parsed = widened as {};",
		"const widened: Record<string, unknown> = { a: 1 }; const parsed = widened as Record<string, unknown> | { a: 1 };",
		"const widened: Record<string, unknown> = { a: 1 }; const parsed = widened as { [key: string]: number };",
		"const widened: Record<string, unknown> = { a: 1 }; const parsed = widened as Map<string, number>;",
		"const widened: Record<string, unknown> = { a: 1 }; const parsed = widened as Record<string, any>;",
		"const widened: Record<string, unknown> = { a: 1 }; const parsed = widened as Readonly<Map<string, number>>;",
		"const widened: Record<boolean, unknown> = { a: 1 }; const parsed = widened as { a: number };",
		"const widened: Record<string> = { a: 1 }; const parsed = widened as { a: number };",
		"const widened: Readonly = { a: 1 }; const parsed = widened as { a: number };",
		"const widened: { [key: string]: number } = { a: 1 }; const parsed = widened as { a: number };",
		"const widened: { a: unknown; b: unknown } = { a: 1, b: 2 }; const parsed = widened as { a: number };",
		"const widened: Box.Value = { a: 1 }; const parsed = widened as { a: number };",
		"const widened = 1 as unknown; const parsed = widened as unknown as number;",
		"const source: unknown = read(); const widened: unknown = source; const parsed = widened as number;",
		"const loop: unknown = loop; const parsed = loop as number;",
		"let source = 1; const widened: unknown = source; const parsed = widened as number;",
		"const parsed = later as number; const later: unknown = 1;",
		"const parsed = missing as number;",
		"const widened: unknown = 1; const parsed = (read() as unknown) as number;",
		"import { source } from 'm'; const widened: unknown = source; const parsed = widened as number;",
		"function f(source) { const widened: unknown = source; return widened as number; }",
		"const source = read() as unknown; const widened: unknown = source; const parsed = widened as number;",
		"const widened: Record = { a: 1 }; const parsed = widened as { a: number };",
		"const widened: Record<string, unknown> = { a: 1 }; const parsed = widened as Readonly;",
		"const widened: Record<string, unknown> = { a: 1 }; const parsed = widened as Record;",
	],
	invalid: [
		{ code: "const widened = { id: 'a' } as unknown; const parsed = widened as { id: string };", errors: [error] },
		{ code: "const widened = <any>`a`; const parsed = <string>widened;", errors: [error] },
		{ code: "const widened: object = [1]; const parsed = <number[]>widened;", errors: [error] },
		{ code: "const widened: object = () => 1; const parsed = widened as readonly string[];", errors: [error] },
		{ code: "const widened: object = new Map(); const parsed = widened as { size: number } & [number];", errors: [error] },
		{ code: "const widened: (object) = class {}; const parsed = widened as new () => object;", errors: [error] },
		{ code: "const widened: object = function () {}; const parsed = widened as { [K in 'a']: number };", errors: [error] },
		{
			code: "const widened: Readonly<Record<string, unknown>> = { a: 1 }; const parsed = widened as Record<string, number>;",
			errors: [error],
		},
		{
			code: "const widened: Record<PropertyKey, any> = { a: 1 }; const parsed = (widened) as Readonly<{ a: 1 }>;",
			errors: [error],
		},
		{
			code: "const widened: Record<string | number | symbol, unknown> = { a: 1 }; const parsed = widened as { a: number };",
			errors: [error],
		},
		{ code: "const widened: { [key: string]: any } = { a: 1 }; const parsed = widened as { a: number };", errors: [error] },
		{
			code: "const source: { id: string } = make(); const widened: unknown = source; const parsed = widened as { id: string };",
			errors: [error],
		},
		{
			code: "const typed = make() as Point; const widened: object = typed; const parsed = widened as Point;",
			errors: [error],
		},
		{
			code: "const first = 1; const second = (first); const widened: unknown = second; const parsed = widened as number;",
			errors: [error],
		},
	],
});
