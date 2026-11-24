import type { JSX } from "@takazudo/zfb/zudo-react/jsx-runtime";
export { createFindInPage } from "./find-in-page.js";
export declare const FIND_IN_PAGE_COMMAND_EVENT = "zudo-doc:find-in-page-command";
export declare function openFindInPage(): void;
export declare function closeFindInPage(): void;
export declare function toggleFindInPage(): void;
export interface FindInPageInitProps {
    /** Disable the package-owned Cmd/Ctrl+F listener when the host dispatches commands. */
    disableBuiltInShortcut?: boolean;
}
export declare function FindInPageInit(props?: FindInPageInitProps): JSX.Element;
export declare namespace FindInPageInit {
    var displayName: string;
}
