/**
 * rename-shell plugin — post-build fixup:
 *
 * Shell page rename: zfb's router skips any page whose filename starts
 * with '_'. The runtime template must live at dist/_shell/index.html so
 * the axum server (S5) can locate it. We build it as dist/shell/index.html
 * and rename here.
 * Workaround for zfb router convention — remove once zfb supports an
 * opt-in escape hatch for underscore-prefixed pages.
 */

import { rename, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";

export default {
  name: "rename-shell",

  async postBuild(ctx) {
    // Move dist/shell/ → dist/_shell/ (runtime template)
    const shellSrc = join(ctx.outDir, "shell");
    const shellDst = join(ctx.outDir, "_shell");
    if (existsSync(shellSrc)) {
      if (!existsSync(shellDst)) {
        await mkdir(shellDst, { recursive: true });
      }
      const indexSrc = join(shellSrc, "index.html");
      const indexDst = join(shellDst, "index.html");
      if (existsSync(indexSrc)) {
        await rename(indexSrc, indexDst);
        ctx.logger.info("rename-shell: moved dist/shell/index.html → dist/_shell/index.html");
      }
    }
  },
};
