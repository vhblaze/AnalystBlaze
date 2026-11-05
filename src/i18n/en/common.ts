// Textos em inglês · common. Mesmas chaves nas duas línguas (o tipo Mirror garante).
// Gerado na etapa 7 do doc 15 a partir do translations.ts antigo; edite à vontade.
import type { Mirror } from "../mirror";
import type { common as pt } from "../pt/common";

export const common: Mirror<typeof pt> = {
  app: {
    versionLine: "v0.1.78 - build 2026.11",
    loading: "Loading...",
  },
  common: {
    cancel: "Cancel",
    close: "Close",
    logout: "Logout",
    refresh: "Refresh",
    refreshing: "Analyzing...",
  },
  ui: {
    advanced: {
      title: "Advanced",
      open: "Open",
      close: "Close",
    },
  },
  confirm: {
    restartWindows: {
      title: "Restart the computer",
      body: "Closes all open programs and restarts Windows in a few seconds. Save what is open first.",
    },
    gameMode: {
      title: "Turn on Game Mode",
      body: "Gives the game priority, lowers background apps' priority, adjusts power and visual effects, runs a reversible cleanup and measures the network. No app is closed. Everything goes back when you turn it off.",
    },
    gameModeOff: {
      title: "Turn off Game Mode",
      body: "Restores what Game Mode changed: power plan, app priorities and visual effects.",
    },
    pcCleanFast: {
      title: "Clean and fast PC",
      body: "Measures the PC, runs a reversible cleanup, adjusts visual effects and background app priority, and measures again. Everything goes to History with Undo.",
    },
    restoreAll: {
      title: "Undo everything that is active",
      body: "Brings power, cleanup, startup apps and services back to the state saved before each change.",
    },
    releaseMemory: {
      title: "Release memory",
      body: "Empties Windows standby memory and programs' idle memory, through the system component. No program is closed. Then we show how much was freed.",
    },
  },
  notFound: {
    title: "404",
    message: "Page not found",
    backHome: "Return home",
  },
};
