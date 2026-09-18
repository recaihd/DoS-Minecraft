import "./styles.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const ip = document.querySelector<HTMLInputElement>("#ip")!;
const port = document.querySelector<HTMLInputElement>("#port")!;
const bots = document.querySelector<HTMLInputElement>("#bots")!;

const startBtn = document.querySelector<HTMLButtonElement>("#start")!;
const stopBtn = document.querySelector<HTMLButtonElement>("#stop")!;

const consoleElement = document.querySelector<HTMLDivElement>("#console")!;

function log(text: string, type = "log") {
  const line = document.createElement("div");
  line.className = type;

  const time = new Date().toLocaleTimeString("pt-BR");
  line.textContent = `[${time}] ${text}`;

  consoleElement.appendChild(line);
  consoleElement.scrollTop = consoleElement.scrollHeight;
}

listen<{ text: string; type: string }>("log", (event) => {
  log(event.payload.text, event.payload.type);
});

listen("finished", () => {
  startBtn.disabled = false;
  stopBtn.disabled = true;
});

async function startReal() {
  const targetIp = ip.value.trim() || "127.0.0.1";
  const targetPort = Number(port.value) || 25565;
  const totalBots = Number(bots.value) || 100;

  startBtn.disabled = true;
  stopBtn.disabled = false;

  log("--------------------------------");
  log(`Target: ${targetIp}:${targetPort}`, "info");
  log(`Bots: ${totalBots}`, "info");
  log("Modo: conexões reais (TCP)", "warning");

  try {
await invoke("start_connections", {
  args: {
    ip: targetIp,
    port: targetPort,
    bots: totalBots,
    delay_ms: 80,
    stay_seconds: 60,   // os players ficaram em media 1 minuto no servidor antes de desconectar
  },
});
  } catch (err) {
    log(`Erro ao iniciar: ${err}`, "error");
    startBtn.disabled = false;
    stopBtn.disabled = true;
  }
}

async function stopReal() {
  try {
    await invoke("stop_connections");
    log("Parando...", "warning");
  } catch (err) {
    log(`Erro ao parar: ${err}`, "error");
  }
}

startBtn.addEventListener("click", startReal);
stopBtn.addEventListener("click", stopReal);

log("ready.", "success");