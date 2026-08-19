/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import { Island } from "@takazudo/zfb";
import { ProbeCounter } from "@/components/probe-counter";

export default function HomePage() {
  return (
    <html lang="en">
      <body>
        <main>
          <h1>CCResDoc node-free compatibility probe</h1>
          <p id="probe-render-state">server-rendered</p>
          <Island when="load">
            <ProbeCounter initial={2} />
          </Island>
          <p><a href="/docs/probe/">Open the representative MDX route</a></p>
        </main>
        <script type="module" src="/assets/islands.js" />
      </body>
    </html>
  );
}
