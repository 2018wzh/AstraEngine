import init from "./astra_player_web.js";
import { createUiComponentSession } from "./astra-ui-component-host.js";

async function startUiComponents() {
  const response = await fetch("./AstraPlayer.config.json", { cache: "no-store" });
  if (!response.ok) throw new Error("ASTRA_WEB_PLAYER_CONFIG_FETCH_FAILED");
  const config = await response.json();
  if (!config.ui_components) return [];
  if (config.ui_components.schema !== "astra.player_ui_components.v1" ||
      !Array.isArray(config.ui_components.components)) {
    throw new Error("ASTRA_WEB_UI_COMPONENT_CONFIG_INVALID");
  }
  const sessions = [];
  for (const entry of config.ui_components.components) {
    if (typeof entry.bindings !== "string" || !entry.core_artifacts ||
        typeof entry.core_artifacts !== "object") {
      throw new Error("ASTRA_WEB_UI_COMPONENT_ENTRY_INVALID");
    }
    const coreArtifacts = new Map(Object.entries(entry.core_artifacts).map(([path, evidence]) => [
      path,
      { sha256: evidence.sha256, byteSize: evidence.byte_size },
    ]));
    const session = await createUiComponentSession({
      bindingsUrl: new URL(entry.bindings, import.meta.url),
      coreArtifacts,
    });
    const empty = new Uint8Array();
    const emptyHash = new Uint8Array(await crypto.subtle.digest("SHA-256", empty));
    session.invoke(1, 1n, BigInt(Date.now()) * 1000000n + 100000000n, emptyHash, empty);
    sessions.push({ session, sequence: 2n, emptyHash });
  }
  return sessions;
}

const launch = document.createElement("button");
launch.type = "button";
launch.textContent = "Start";
launch.dataset.astraPermissionHandshake = "pending";
document.body.appendChild(launch);

launch.addEventListener("click", async () => {
  launch.disabled = true;
  try {
    const uiComponents = await startUiComponents();
    await init("./astra_player_web_bg.wasm");
    globalThis.__astraUiComponentSessions = uiComponents;
    addEventListener("pagehide", () => {
      for (const state of uiComponents) {
        state.session.invoke(5, state.sequence++, BigInt(Date.now()) * 1000000n + 100000000n, state.emptyHash, new Uint8Array());
      }
    }, { once: true });
    launch.dataset.astraPermissionHandshake = "complete";
    launch.remove();
  } catch (error) {
    launch.disabled = false;
    launch.dataset.astraPermissionHandshake = "blocked";
    throw error;
  }
}, { once: false });
