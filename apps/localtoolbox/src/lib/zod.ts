// zod, as every schema in the popup should import it.
//
// zod compiles an object schema into a function with `new Function` wherever it
// is allowed to, and finds out whether it is by trying. An extension page's
// content security policy refuses it, and Chrome reports every refusal on the
// extension's errors page even though zod catches it, so the probe is turned
// off here before any schema runs. `zod/mini` rather than `zod`: the popup
// boots from one small chunk, and this API tree-shakes down to what is used.

import * as z from 'zod/mini';

z.config({ jitless: true });

/**
 * `z.catch` under a name of its own: a schema that answers `value` for any input it refuses.
 * promise/valid-params reads every two-argument `.catch()` as a misused Promise#catch.
 */
export const fallback = z.catch;

export { z };
