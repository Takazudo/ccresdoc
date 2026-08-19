import { zudoDoc } from "@takazudo/zudo-doc/config";
import { probeSettings } from "./settings.mjs";

export default zudoDoc({ ...probeSettings, packageOwnedRoutes: false });
