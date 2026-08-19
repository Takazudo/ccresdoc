import { zudoDoc } from "@takazudo/zudo-doc/config";
import { probeSettings } from "./settings.mjs";

// Maximum first-party adoption compatible with CCResDoc's runtime invariant:
// retain the package's collections/schema/Markdown defaults, but host routes
// locally and remove every Node plugin-host descriptor after composition.
export default {
  ...zudoDoc({ ...probeSettings, packageOwnedRoutes: false }),
  plugins: [],
};
