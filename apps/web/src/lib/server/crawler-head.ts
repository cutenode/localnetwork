/**
 * Where the server writes a page's tags that only crawlers read, like its schema.org structured data: a comment in
 * app.html, ahead of anything a page renders, so the first one is always the template's. Written by the server hook
 * rather than a component, the code that builds them stays out of the JavaScript every page loads.
 */
export const CRAWLER_HEAD = '<!--crawler-head-->';

/**
 * `html` with `head` in place of the marker. A replacer function, so a `$&` someone wrote into their name stays as
 * written rather than reading as a replacement pattern.
 */
export const withCrawlerHead = (html: string, head = '') => html.replace(CRAWLER_HEAD, () => head);
