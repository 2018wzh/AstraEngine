import init from "./astra_player_web.js";

const launch = document.createElement("button");
launch.type = "button";
launch.textContent = "Start";
launch.dataset.astraPermissionHandshake = "pending";
document.body.appendChild(launch);

launch.addEventListener("click", async () => {
  launch.disabled = true;
  try {
    await init("./astra_player_web_bg.wasm");
    launch.dataset.astraPermissionHandshake = "complete";
    launch.remove();
  } catch (error) {
    launch.disabled = false;
    launch.dataset.astraPermissionHandshake = "blocked";
    throw error;
  }
}, { once: false });
