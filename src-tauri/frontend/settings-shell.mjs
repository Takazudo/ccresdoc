import { createTauriBackend, decodeBackendError } from "./settings-backend.mjs";

const loading = document.getElementById("settings-loading");
const content = document.getElementById("settings-content");
const fatal = document.getElementById("settings-fatal");
const fatalMessage = document.getElementById("fatal-message");

function show(target) {
  for (const section of [loading, content, fatal]) section.hidden = section !== target;
}

async function load() {
  show(loading);
  try {
    const snapshot = await createTauriBackend().getSnapshot();
    document.getElementById("config-path").textContent = snapshot.settings.configPath;
    document.getElementById("config-status").textContent = snapshot.settings.status;
    document.getElementById("runtime-status").textContent = snapshot.runtime.phase;
    show(content);
  } catch (error) {
    fatalMessage.textContent = decodeBackendError(error).message;
    show(fatal);
  }
}

document.getElementById("reload-settings").addEventListener("click", load);
load();
