"use client";

import { useState } from "preact/hooks";

export function ProbeCounter({ initial }: { initial: number }) {
  const [count, setCount] = useState(initial);
  return (
    <button type="button" data-probe-counter onClick={() => setCount((value) => value + 1)}>
      Count: {count}
    </button>
  );
}

ProbeCounter.displayName = "ProbeCounter";
