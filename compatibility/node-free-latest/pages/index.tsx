import { Island } from "@takazudo/zfb";
import { ProbeCounter } from "@/components/probe-counter";

export default function HomePage() {
  return (
    <html lang="en">
      <head>
        <meta charset="utf-8" />
        <title>CCResDoc node-free compatibility probe</title>
      </head>
      <body>
        <main>
          <h1>CCResDoc node-free compatibility probe</h1>
          <p id="probe-render-state">server-rendered</p>
          <Island when="load">
            <ProbeCounter initial={2} />
          </Island>
          <p><a href="/docs/probe/">Open the representative MDX route</a></p>
        </main>
      </body>
    </html>
  );
}
