import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

const rootEl = document.getElementById("root") as HTMLElement;

function showFatal(msg: string) {
  rootEl.innerHTML = `<div style="font:14px/1.4 system-ui;padding:24px;color:#c23b3b;white-space:pre-wrap">DeskCap UI 启动失败\n${msg}</div>`;
}

async function ensureWindowSize() {
  try {
    const win = getCurrentWindow();
    await win.setMinSize(new LogicalSize(960, 640));
    await win.setSize(new LogicalSize(1280, 800));
    await win.center();
    await win.unminimize();
    await win.show();
    await win.setFocus();
  } catch (e) {
    console.warn("ensureWindowSize", e);
  }
}

async function boot() {
  // Prove JS + IPC before React mounts (blank WebView diagnosis).
  try {
    await invoke("mark_ui_ready");
  } catch (e) {
    showFatal(`IPC mark_ui_ready: ${String(e)}`);
    return;
  }

  await ensureWindowSize();

  try {
    const { default: App } = await import("./App");
    ReactDOM.createRoot(rootEl).render(
      <React.StrictMode>
        <App />
      </React.StrictMode>,
    );
    // Again after React paint — some macOS icon/resource glitches shrink the frame once.
    window.setTimeout(() => {
      void ensureWindowSize();
    }, 300);
  } catch (e) {
    showFatal(String(e));
  }
}

boot();
