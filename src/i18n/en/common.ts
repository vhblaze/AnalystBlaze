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
    logout: "Sign out",
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
    restoreGameMode: {
      title: "Turn off Game Mode",
      body: "Restores what the Game Mode session changed: power plan, app priorities and visual effects.",
    },
    deepTemp: {
      title: "Deep cleanup of temporary files",
      body: "Moves temporary files unused for at least 5 minutes to quarantine. With the system component, includes the Windows temp folder. Can be undone from the quarantine.",
    },
    purgeQuarantine: {
      title: "Empty the quarantine",
      body: "Permanently deletes everything in the quarantine to free the space. After that, those files do not come back.",
    },
    enableDefrag: {
      title: "Turn automatic disk optimization back on",
      body: "Turns the Windows task that defragments the HDD from time to time back on, keeping reads fast. Needs the system component.",
    },
    sfc: {
      title: "Check Windows files",
      body: "Runs Windows' own system file check. It can take several minutes. Do not shut down or restart the PC while it runs.",
    },
    dism: {
      title: "Repair Windows components",
      body: "Runs Windows' own component repair. Takes longer than the file check and needs internet. Do not shut down or restart the PC while it runs.",
    },
    restartDevice: {
      title: "Restart the device",
      body: "Turns the device off and on again, like in Device Manager. Fixes a good share of driver errors; some only go away by restarting Windows.",
    },
    gameDvr: {
      title: "Turn off Game Bar recording",
      body: "Turns off Xbox Game Bar background recording. Can be undone: the previous value stays in History.",
    },
    repairService: {
      title: "Try to fix the service",
      body: "Stops and restarts the service, then checks it stayed up before calling it fixed. May not fix deeper causes.",
    },
    disableService: {
      title: "Turn the service off for good",
      body: "The service stops starting on its own. Can be undone from History. Only use it if you are sure you do not need it.",
    },
    defenderThrottle: {
      title: "Limit Defender scans",
      body: "Lowers the CPU scheduled Defender scans may use (from 50% to 20%) and lets them run only while the PC is idle. Real-time protection does not change. Can be undone from the same card.",
    },
    defenderRestore: {
      title: "Restore Defender scans",
      body: "Puts Defender's scan preferences back exactly as they were.",
    },
    defenderRenew: {
      title: "Renew Defender definitions",
      body: "Removes the current virus definition set (which may be corrupt) and downloads a fresh one. Defender runs without definitions for a few minutes; do not open suspicious files meanwhile.",
    },
    volumeCheck: {
      title: "Check drive {{drive}}: on the next restart",
      body: "Marks the drive so Windows checks and repairs the file system before it opens, next time you restart. That boot can take a few minutes longer.",
    },
    disableStartup: {
      title: "Remove {{name}} from startup",
      body: "Removes the app from Windows startup ({{location}}) and keeps how it was so it comes back with one click.",
      unknownLocation: "detected registry entry",
    },
    restoreStartup: {
      title: "Put {{name}} back in startup",
      titleAll: "Put the apps back in startup",
      body: "Restores the value kept before the change.",
    },
    delayStartup: {
      title: "Delay {{name}}",
      body: "Takes {{name}} out of direct startup and puts it in the AnalystBlaze queue. Today the queue does not reopen the app on its own.",
    },
    restoreDelayedStartup: {
      title: "Put {{name}} back in startup",
      titleAll: "Put delayed apps back in startup",
      body: "Restores the startup value kept before the change.",
    },
    stopService: {
      title: "Stop the {{name}} service",
      body: "Only stops if the service is not on the critical list. Before that, it records whether it was running.",
    },
    restoreService: {
      title: "Start the {{name}} service again",
      titleAll: "Start the services again",
      body: "Only starts services that were running before the action.",
    },
    powerPlan: {
      high: "Turn on high performance",
      saver: "Turn on power saver",
      balanced: "Turn on the balanced plan",
      body: "Switches the Windows power plan and keeps the previous one to go back later.",
    },
    visualPerformance: {
      title: "Turn on performance visuals",
      body: "Reduces Windows animations, transparency and visual effects. Can be undone. Other open programs may hitch for an instant at the switch; that is Windows announcing the change, not an error.",
    },
    restoreVisual: {
      title: "Restore visual effects",
      body: "Puts Windows visual effects back as they were before the last change.",
    },
    restorePerformanceSession: {
      title: "Undo Clean and fast PC",
      body: "Reverts what the last Clean and fast PC session changed: visual effects, priorities and startup.",
    },
    cleanupCategory: {
      title: "Clean this category",
      body: "Moves this category's files to quarantine. Can be undone until the quarantine is emptied.",
    },
    startAgent: {
      title: "Connect the agent",
      body: "Connects the local agent and starts measuring.",
    },
    collectSample: {
      title: "Measure now",
      body: "Takes a reading of the PC now.",
    },
    login: {
      title: "Sign in on the site",
      body: "Opens the AnalystBlaze sign-in in the browser.",
    },
    flushDns: {
      title: "Clear the DNS cache",
      body: "Empties the list of addresses Windows kept. Needs no administrator and changes nothing that would need undoing.",
    },
    setDns: {
      title: "Change the adapter's DNS",
      body: "Switches the chosen adapter's DNS servers. The current setting is kept to go back later.",
    },
    resetWinsock: {
      title: "Reset Windows networking (Winsock)",
      body: "Resets the Windows network layer. Requires a PC restart and cannot be undone by AnalystBlaze.",
    },
    adapterInUse: {
      title: "Warning: this interface seems to be in use",
    },
    adapterEnable: {
      title: "Turn the network adapter on",
      body: "Turns the chosen network adapter back on.",
    },
    adapterDisable: {
      title: "Turn the network adapter off",
      body: "Turns the chosen adapter off so traffic leaves through another one. The current state is kept to go back later.",
    },
    networkTune: {
      title: "Apply the network tune",
      body: "Adjusts Windows TCP parameters. The current setting is kept; if you do not confirm it got better, it reverts on its own.",
    },
    restartForNetwork: {
      title: "Restart the computer",
      body: "Closes all open programs and restarts Windows in a few seconds. Needed for the network change to take effect.",
    },
  },
  repairPopup: {
    elapsed: "Running for {{elapsed}}",
    background: "Keep going in the background",
    close: "Close",
    doneTitle: "Check finished",
    failedTitle: "Could not run the check",
    runDism: "Repair with DISM",
    counterNote: "The critical-errors counter comes from the Windows log and only drops when the errors leave the last 24 h.",
    sfc: {
      runningTitle: "Checking system files",
      sub: "Windows checks the protected files one by one. It takes 5 to 15 minutes.",
      step1: "Reads the protected Windows files",
      step2: "Compares each one with the copy Windows keeps",
      step3: "Repairs what differs, if anything",
      note: "Don't restart or shut down the PC. You can keep using it; Windows doesn't report a percentage, so what shows here is the time.",
    },
    dism: {
      runningTitle: "Repairing Windows components",
      sub: "DISM checks the component store and fixes what's damaged. It takes 10 to 20 minutes and needs internet.",
      step1: "Checks the Windows component store",
      step2: "Downloads what's damaged from Windows Update",
      step3: "Replaces the corrupted components",
      note: "Don't restart or shut down the PC. After DISM, run the system check again.",
    },
  },
  pcCleanFastRun: {
    runningTitle: "Applying clean and fast PC",
    runningSub: "Each step is a measured rule and is saved so it can be undone.",
    stepMeasure: "Measures the current state",
    stepClean: "Cleans temp files (they go to quarantine)",
    stepStartup: "Delays startup apps",
    stepBackground: "Lowers background app priority",
    stepMeasureAgain: "Measures the result",
    runningNote: "It can take a few minutes. You can keep using the PC; don't close AnalystBlaze. No new window opens.",
    elapsed: "{{seconds}} s",
    doneTitle: "Done",
    doneSub: "Measured again right after applying.",
    scoreLine: "Score {{before}} → {{after}}",
    scoreUnavailable: "The agent didn't return the score this time. What was done is in History.",
    appliedHeading: "What was done",
    nothingApplied: "Nothing needed to change on this PC.",
    undo: "Undo everything",
    close: "Close",
    failedTitle: "Couldn't apply it",
    failedSub: "Nothing was left half done: whatever was already changed is in History, with Undo.",
    stepNames: {
      APPLY_CLEANUP_CATEGORY: "Temp files moved to quarantine",
      CLEAR_STANDBY_LIST: "Standby memory released",
      APPLY_VISUAL_PERFORMANCE_MODE: "Visual effects adjusted",
      APPLY_PC_CLEAN_FAST_BACKGROUND_PRIORITIES: "Background app priority lowered",
      APPLY_GAME_MODE: "Game Mode turned on",
      DELAY_STARTUP_APP: "Startup app delayed",
      NETWORK_DIAGNOSTICS: "Network checked",
    },
  },
  notFound: {
    title: "404",
    message: "Page not found",
    backHome: "Return home",
  },
};
