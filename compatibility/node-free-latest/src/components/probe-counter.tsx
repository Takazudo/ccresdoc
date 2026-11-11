"use client";

import { signal } from "@takazudo/zfb/zudo-react";

export function ProbeCounter({ initial }: { initial: number }) {
  const count = signal(initial);
  return (
    <button type="button" data-probe-counter on:click={() => { count.value += 1; }}>
      Count: {count}
    </button>
  );
}

ProbeCounter.displayName = "ProbeCounter";
