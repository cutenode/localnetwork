// Before every server and integration test file: the lexicons ours reference, as stand-ins, so no test fetches them.
import { loadReferencedLexicons } from '../atproto/lexicons';
import { standInLexicons } from './lexicons';

await loadReferencedLexicons(standInLexicons);
