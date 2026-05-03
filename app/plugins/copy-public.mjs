/**
 * copy-public plugin — two post-build fixups:
 *
 * 1. Public-dir copy: zfb does not copy the public/ directory to dist/
 *    during production builds (public/ is a dev-server-only static root).
 *    This hook fills the gap by mirroring public/** into dist/.
 *    Workaround for missing zfb feature — remove once zfb handles this natively.
 *
 * 2. Shell page rename: zfb's router skips any page whose filename starts
 *    with '_'. The runtime template must live at dist/_shell/index.html so
 *    the axum server (S5) can locate it. We build it as dist/shell/index.html
 *    and rename here.
 *    Workaround for zfb router convention — remove once zfb supports an
 *    opt-in escape hatch for underscore-prefixed pages.
 */

import { cp, rename, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";

export default {
  name: "copy-public",

  async postBuild(ctx) {
    // 1. Copy public/ → dist/
    const publicDir = join(ctx.projectRoot, "public");
    if (!existsSync(publicDir)) {
      ctx.logger.info("copy-public: no public/ dir found, skipping copy");
    } else {
      await cp(publicDir, ctx.outDir, { recursive: true });
      ctx.logger.info("copy-public: copied public/ → dist/");
    }

    // 2. Move dist/shell/ → dist/_shell/ (runtime template)
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
        ctx.logger.info("copy-public: moved dist/shell/index.html → dist/_shell/index.html");
      }
    }
  },
};
