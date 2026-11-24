export interface FindResult {
    matches: number;
    activeMatchOrdinal: number;
}
export interface FindInPage {
    find(container: HTMLElement, query: string): FindResult;
    next(): FindResult;
    prev(): FindResult;
    stop(): void;
}
/**
 * Create a DOM-based find-in-page utility.
 *
 * Limitation: only matches within single text nodes. Cross-element matching
 * (e.g. "Hello world" spanning `<strong>Hello</strong> world`) is not supported.
 */
export declare function createFindInPage(): FindInPage;
