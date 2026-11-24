/** @jsxRuntime automatic */
import { type ReadonlySignal } from "@takazudo/zfb/zudo-react";
import type { FindInPage } from "./find-in-page.js";
interface FindBarProps {
    visible: ReadonlySignal<boolean>;
    /** Incremented by a host open command to refocus and select an already-open bar. */
    focusRequest?: ReadonlySignal<number>;
    onClose: () => void;
    findInPage: FindInPage;
    containerSelector: string;
}
export declare function FindBar({ visible, focusRequest, onClose, findInPage, containerSelector }: FindBarProps): import("@takazudo/zfb").Description;
export {};
