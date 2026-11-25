// Invoked by osascript's JavaScript for Automation runtime.
ObjC.import("AppKit");
ObjC.import("ApplicationServices");

function matchingApps() {
  return $.NSRunningApplication.runningApplicationsWithBundleIdentifier("com.takazudo.ccresdoc");
}

function run([action, bundle, pid]) {
  if (action === "accessibility") return $.AXIsProcessTrusted() ? "trusted" : "unavailable";

  const apps = matchingApps();
  if (action === "list") return String(apps.count);

  const pids = [];
  for (let i = 0; i < apps.count; i++) {
    const app = apps.objectAtIndex(i);
    const path = ObjC.unwrap(app.bundleURL.URLByResolvingSymlinksInPath.path);
    if (path !== bundle) continue;
    pids.push(app.processIdentifier);
    if (action === "quit" && app.processIdentifier === Number(pid)) {
      return app.terminate ? "requested" : "refused";
    }
  }
  return action === "pids" ? pids.join("\n") : "missing";
}
