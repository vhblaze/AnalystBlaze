/** Textos de Meu PC › Saúde do Windows e das specs do header (mypc.health, mypc.specs). */
export const healthStrings = {
  "pt-BR": {
    intro: "O que está errado no Windows e não é sobre desempenho. Cada item diz o que fazer.",
    ok: "ok",
    noReading: "Ainda sem leitura do Windows. Ela chega com a próxima amostra do agente.",
    noAdvanced: "O agente ainda não mandou os diagnósticos do Windows.",
    foot: "Os avisos ficam aqui, sem notificação. O Defender e o modem USB ainda aparecem em Insights.",
    openInsights: "Abrir Insights",
    actions: {
      restart: "Reiniciar agora",
      sfc: "Verificar arquivos do sistema",
      restartDevice: "Reiniciar dispositivo",
    },
    items: {
      reboot: {
        title: "Reinício pendente",
        detail: "Uma atualização do Windows foi instalada e só termina quando o PC reiniciar. Até lá o sistema pode ficar mais lento.",
        okTitle: "Atualizações do Windows",
        okDetail: "Nenhum reinício pendente.",
      },
      shadow: {
        title: "Pontos de restauração sendo apagados",
        detail: "O espaço reservado chegou ao limite e o Windows apaga os pontos antigos. Você decide se o AnalystBlaze pode aumentar o limite.",
      },
      services: {
        title: "Serviços do Windows falhando: {{count}}",
        detail: "{{names}}. Pode indicar arquivos do sistema corrompidos; a verificação confirma.",
        detailWithCrashes: "{{names}}. Mais {{crashes}} travamentos do Windows. Pode indicar arquivos do sistema corrompidos; a verificação confirma.",
        okTitle: "Serviços e eventos do Windows",
        okDetail: "Nenhum serviço falhando. {{events}} erros críticos nas últimas 24 h.",
      },
      events: {
        title: "Erros críticos nas últimas 24 h: {{count}}",
      },
      device: {
        title: "{{name}} com problema",
        okTitle: "Dispositivos",
        okDetail: "Nenhum dispositivo com erro no Gerenciador de Dispositivos.",
      },
      codes: {
        cannotStart: "O dispositivo não consegue iniciar (código 10).",
        noResources: "Recursos de hardware insuficientes (código 12).",
        needsRestart: "Precisa reiniciar o Windows para funcionar (código 14).",
        reinstallDrivers: "Os drivers precisam ser reinstalados (código 18).",
        registryProblem: "Configuração do Registro corrompida (código 19).",
        driversNotInstalled: "Drivers não instalados (código 28).",
        couldNotStart: "O Windows não conseguiu iniciar o dispositivo (código 31).",
        driverError: "O driver retornou um erro (código 37).",
        previousInstance: "Uma instância anterior do driver ainda está carregada (código 38).",
        driverMissing: "Driver corrompido ou ausente (código 39).",
        registryEntryCorrupt: "Entrada do Registro do driver corrompida (código 40).",
        stoppedByWindows: "O Windows parou o dispositivo por um problema reportado (código 43).",
        blocked: "Driver bloqueado por incompatibilidade (código 48).",
        other: "O Windows reportou o código {{code}} para este dispositivo.",
      },
    },
  },
  "en-US": {
    intro: "What is wrong in Windows that is not about performance. Each item says what to do.",
    ok: "ok",
    noReading: "No Windows reading yet. It arrives with the agent's next sample.",
    noAdvanced: "The agent has not sent the Windows diagnostics yet.",
    foot: "Warnings stay here, without notifications. Defender and the USB modem still show up in Insights.",
    openInsights: "Open Insights",
    actions: {
      restart: "Restart now",
      sfc: "Check system files",
      restartDevice: "Restart device",
    },
    items: {
      reboot: {
        title: "Restart pending",
        detail: "A Windows update was installed and only finishes when the PC restarts. Until then the system may be slower.",
        okTitle: "Windows updates",
        okDetail: "No restart pending.",
      },
      shadow: {
        title: "Restore points being deleted",
        detail: "The reserved space hit its limit and Windows deletes old points. You decide whether AnalystBlaze may raise the limit.",
      },
      services: {
        title: "Windows services failing: {{count}}",
        detail: "{{names}}. May point to corrupted system files; the check confirms it.",
        detailWithCrashes: "{{names}}. Plus {{crashes}} Windows crashes. May point to corrupted system files; the check confirms it.",
        okTitle: "Windows services and events",
        okDetail: "No service failing. {{events}} critical errors in the last 24 h.",
      },
      events: {
        title: "Critical errors in the last 24 h: {{count}}",
      },
      device: {
        title: "{{name}} has a problem",
        okTitle: "Devices",
        okDetail: "No device with errors in Device Manager.",
      },
      codes: {
        cannotStart: "The device cannot start (code 10).",
        noResources: "Not enough hardware resources (code 12).",
        needsRestart: "Windows needs a restart for it to work (code 14).",
        reinstallDrivers: "The drivers need to be reinstalled (code 18).",
        registryProblem: "Registry configuration is corrupted (code 19).",
        driversNotInstalled: "Drivers not installed (code 28).",
        couldNotStart: "Windows could not start the device (code 31).",
        driverError: "The driver returned an error (code 37).",
        previousInstance: "A previous instance of the driver is still loaded (code 38).",
        driverMissing: "Driver corrupted or missing (code 39).",
        registryEntryCorrupt: "The driver's registry entry is corrupted (code 40).",
        stoppedByWindows: "Windows stopped the device because of a reported problem (code 43).",
        blocked: "Driver blocked for incompatibility (code 48).",
        other: "Windows reported code {{code}} for this device.",
      },
    },
  },
} as const;

export const specsStrings = {
  "pt-BR": { uptime: "ligado há {{time}}" },
  "en-US": { uptime: "on for {{time}}" },
} as const;
