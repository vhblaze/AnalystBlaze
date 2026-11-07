// Textos em português · common. Mesmas chaves nas duas línguas.
// Gerado na etapa 7 do doc 15 a partir do translations.ts antigo; edite à vontade.
export const common = {
  app: {
    versionLine: "v0.2.0 - build 2026.11",
    loading: "Carregando...",
  },
  common: {
    cancel: "Cancelar",
    close: "Fechar",
    logout: "Sair",
    refresh: "Atualizar",
    refreshing: "Analisando...",
  },
  ui: {
    advanced: {
      title: "Avançado",
      open: "Abrir",
      close: "Fechar",
    },
  },
  confirm: {
    restartWindows: {
      title: "Reiniciar o computador",
      body: "Fecha todos os programas abertos e reinicia o Windows em alguns segundos. Salve o que estiver aberto antes.",
    },
    gameMode: {
      title: "Ativar Modo Jogo",
      body: "Dá prioridade ao jogo, baixa a prioridade dos apps em segundo plano, ajusta energia e efeitos visuais, faz uma limpeza reversível e mede a rede. Nenhum app é fechado. Tudo volta ao desligar.",
    },
    gameModeOff: {
      title: "Desligar Modo Jogo",
      body: "Restaura o que o Modo Jogo mudou: plano de energia, prioridades dos apps e efeitos visuais.",
    },
    pcCleanFast: {
      title: "PC limpo e rápido",
      body: "Mede o PC, faz uma limpeza reversível, ajusta efeitos visuais e a prioridade dos apps em segundo plano, e mede de novo. Tudo fica no Histórico com Desfazer.",
    },
    restoreAll: {
      title: "Desfazer tudo o que está ativo",
      body: "Volta energia, limpeza, apps de inicialização e serviços ao estado salvo antes de cada mudança.",
    },
    releaseMemory: {
      title: "Liberar memória",
      body: "Esvazia a memória em espera do Windows e a memória ociosa dos programas, pelo componente de sistema. Nenhum programa é fechado. Depois mostramos quanto foi liberado.",
    },
    restoreGameMode: {
      title: "Desligar Modo Jogo",
      body: "Restaura o que a sessão do Modo Jogo mudou: plano de energia, prioridades dos apps e efeitos visuais.",
    },
    deepTemp: {
      title: "Limpeza profunda de temporários",
      body: "Move para a quarentena os arquivos temporários que não estão em uso há pelo menos 5 minutos. Com o componente de sistema, inclui a pasta temporária do Windows. Dá para desfazer pela quarentena.",
    },
    purgeQuarantine: {
      title: "Esvaziar a quarentena",
      body: "Apaga de vez tudo o que está na quarentena para liberar o espaço. Depois disso, esses arquivos não voltam.",
    },
    enableDefrag: {
      title: "Religar a otimização automática do disco",
      body: "Religa a tarefa do Windows que desfragmenta o HD de tempos em tempos e mantém a leitura rápida. Precisa do componente de sistema.",
    },
    sfc: {
      title: "Verificar os arquivos do Windows",
      body: "Roda a verificação de arquivos do sistema do próprio Windows. Pode levar vários minutos. Não desligue nem reinicie o PC enquanto roda.",
    },
    dism: {
      title: "Reparar os componentes do Windows",
      body: "Roda o reparo de componentes do próprio Windows. Leva mais tempo que a verificação de arquivos e precisa de internet. Não desligue nem reinicie o PC enquanto roda.",
    },
    restartDevice: {
      title: "Reiniciar o dispositivo",
      body: "Desliga e religa o dispositivo, como no Gerenciador de Dispositivos. Resolve boa parte dos erros de driver; alguns só somem reiniciando o Windows.",
    },
    gameDvr: {
      title: "Desligar a gravação do Game Bar",
      body: "Desliga a gravação em segundo plano do Xbox Game Bar. Dá para desfazer: o valor anterior fica no Histórico.",
    },
    repairService: {
      title: "Tentar corrigir o serviço",
      body: "Para e reinicia o serviço, e depois confere se ele continuou ativo antes de dizer que deu certo. Pode não resolver causas mais profundas.",
    },
    disableService: {
      title: "Desligar o serviço de vez",
      body: "O serviço deixa de iniciar sozinho. Dá para desfazer pelo Histórico. Use só se tiver certeza de que não precisa dele.",
    },
    defenderThrottle: {
      title: "Limitar as verificações do Defender",
      body: "Reduz a CPU que as verificações agendadas do Defender podem usar (de 50% para 20%) e deixa que rodem só com o PC parado. A proteção em tempo real não muda. Dá para desfazer pelo próprio card.",
    },
    defenderRestore: {
      title: "Restaurar as verificações do Defender",
      body: "Devolve as preferências de verificação do Defender exatamente como estavam antes.",
    },
    defenderRenew: {
      title: "Renovar as definições do Defender",
      body: "Remove o conjunto atual de definições de vírus (que pode estar corrompido) e baixa um novo. O Defender fica alguns minutos sem definições até terminar; não abra arquivos suspeitos nesse intervalo.",
    },
    volumeCheck: {
      title: "Verificar o disco {{drive}}: na próxima reinicialização",
      body: "Marca a unidade para o Windows verificar e reparar o sistema de arquivos antes de abrir, na próxima vez que você reiniciar. Esse boot pode levar alguns minutos a mais.",
    },
    disableStartup: {
      title: "Tirar {{name}} da inicialização",
      body: "Remove o app da inicialização do Windows ({{location}}) e guarda como estava para voltar com um clique.",
      unknownLocation: "registro detectado",
    },
    restoreStartup: {
      title: "Voltar {{name}} para a inicialização",
      titleAll: "Voltar os apps para a inicialização",
      body: "Restaura o valor guardado antes da mudança.",
    },
    delayStartup: {
      title: "Atrasar {{name}}",
      body: "Tira {{name}} da inicialização direta e coloca na fila do AnalystBlaze. Hoje a fila não reabre o app sozinho.",
    },
    restoreDelayedStartup: {
      title: "Voltar {{name}} para a inicialização",
      titleAll: "Voltar os apps atrasados para a inicialização",
      body: "Restaura o valor de inicialização guardado antes da mudança.",
    },
    stopService: {
      title: "Parar o serviço {{name}}",
      body: "Só para se o serviço não estiver na lista de críticos. Antes, guarda se ele estava rodando.",
    },
    restoreService: {
      title: "Religar o serviço {{name}}",
      titleAll: "Religar os serviços",
      body: "Religa só os serviços que estavam rodando antes da ação.",
    },
    powerPlan: {
      high: "Ativar alto desempenho",
      saver: "Ativar economia de energia",
      balanced: "Ativar plano equilibrado",
      body: "Troca o plano de energia do Windows e guarda o anterior para voltar depois.",
    },
    visualPerformance: {
      title: "Ativar visual de desempenho",
      body: "Reduz animações, transparência e efeitos visuais do Windows. Dá para desfazer. Outros programas abertos podem dar um engasgo de um instante na troca; é o Windows avisando a mudança, não um erro.",
    },
    restoreVisual: {
      title: "Restaurar os efeitos visuais",
      body: "Volta os efeitos visuais do Windows como estavam antes da última mudança.",
    },
    restorePerformanceSession: {
      title: "Desfazer PC limpo e rápido",
      body: "Volta o que a última sessão de PC limpo e rápido mudou: efeitos visuais, prioridades e inicialização.",
    },
    cleanupCategory: {
      title: "Limpar esta categoria",
      body: "Move os arquivos desta categoria para a quarentena. Dá para desfazer até esvaziar a quarentena.",
    },
    startAgent: {
      title: "Conectar o agente",
      body: "Conecta o agente local e começa a medir.",
    },
    collectSample: {
      title: "Medir agora",
      body: "Faz uma leitura do PC agora.",
    },
    login: {
      title: "Entrar pelo site",
      body: "Abre o login do AnalystBlaze no navegador.",
    },
    flushDns: {
      title: "Limpar o cache de DNS",
      body: "Esvazia a lista de endereços que o Windows guardou. Não precisa de administrador e não muda nada que precise ser desfeito.",
    },
    setDns: {
      title: "Trocar o DNS do adaptador",
      body: "Troca os servidores DNS do adaptador escolhido. A configuração atual fica guardada para voltar depois.",
    },
    resetWinsock: {
      title: "Redefinir a rede do Windows (Winsock)",
      body: "Redefine a camada de rede do Windows. Exige reiniciar o PC e não dá para desfazer pelo AnalystBlaze.",
    },
    adapterInUse: {
      title: "Atenção: esta interface parece estar em uso",
    },
    adapterEnable: {
      title: "Ligar o adaptador de rede",
      body: "Liga de novo o adaptador de rede escolhido.",
    },
    adapterDisable: {
      title: "Desligar o adaptador de rede",
      body: "Desliga o adaptador escolhido para o tráfego sair por outro. O estado atual fica guardado para voltar depois.",
    },
    networkTune: {
      title: "Aplicar o ajuste de rede",
      body: "Ajusta parâmetros de TCP do Windows. A configuração atual fica guardada; se você não confirmar que ficou bom, volta sozinha.",
    },
    restartForNetwork: {
      title: "Reiniciar o computador",
      body: "Fecha todos os programas abertos e reinicia o Windows em alguns segundos. É preciso para o ajuste de rede valer.",
    },
  },
  repairPopup: {
    elapsed: "Em andamento há {{elapsed}}",
    background: "Continuar em segundo plano",
    close: "Fechar",
    doneTitle: "Verificação concluída",
    failedTitle: "Não deu para verificar",
    runDism: "Reparar com DISM",
    counterNote: "O contador de erros críticos vem do log do Windows e só diminui quando os erros saem das últimas 24 h.",
    sfc: {
      runningTitle: "Verificando arquivos do sistema",
      sub: "O Windows confere os arquivos protegidos um a um. Leva de 5 a 15 minutos.",
      step1: "Lê os arquivos protegidos do Windows",
      step2: "Compara cada um com a cópia guardada pelo próprio Windows",
      step3: "Repara o que estiver diferente, se houver",
      note: "Não reinicie nem desligue o PC. Pode continuar usando; o Windows não informa a porcentagem, então o que aparece aqui é o tempo.",
    },
    dism: {
      runningTitle: "Reparando componentes do Windows",
      sub: "O DISM confere o repositório de componentes e corrige o que estiver danificado. Leva de 10 a 20 minutos e precisa de internet.",
      step1: "Confere o repositório de componentes do Windows",
      step2: "Baixa do Windows Update o que estiver danificado",
      step3: "Substitui os componentes corrompidos",
      note: "Não reinicie nem desligue o PC. Depois do DISM, rode a verificação do sistema de novo.",
    },
  },
  pcCleanFastRun: {
    runningTitle: "Aplicando PC limpo e rápido",
    runningSub: "Cada passo é uma regra medida e fica guardado para desfazer.",
    stepMeasure: "Mede o estado atual",
    stepClean: "Limpa temporários (vão para a quarentena)",
    stepStartup: "Adia apps de inicialização",
    stepBackground: "Reduz a prioridade dos apps em segundo plano",
    stepMeasureAgain: "Mede o resultado",
    runningNote: "Pode levar alguns minutos. Pode continuar usando o PC; não feche o AnalystBlaze. Nada abre janela nova.",
    elapsed: "{{seconds}} s",
    doneTitle: "Pronto",
    doneSub: "Medido de novo logo após aplicar.",
    scoreLine: "Score {{before}} → {{after}}",
    scoreUnavailable: "O agente não devolveu o score desta vez. O que foi feito está no Histórico.",
    appliedHeading: "O que foi feito",
    nothingApplied: "Nada precisou mudar neste PC.",
    undo: "Desfazer tudo",
    close: "Fechar",
    failedTitle: "Não deu para aplicar",
    failedSub: "Nada ficou pela metade: o que já tinha sido mudado está no Histórico, com Desfazer.",
    stepNames: {
      APPLY_CLEANUP_CATEGORY: "Temporários movidos para a quarentena",
      CLEAR_STANDBY_LIST: "Memória em espera liberada",
      APPLY_VISUAL_PERFORMANCE_MODE: "Efeitos visuais ajustados",
      APPLY_PC_CLEAN_FAST_BACKGROUND_PRIORITIES: "Prioridade dos apps em segundo plano reduzida",
      APPLY_GAME_MODE: "Modo Jogo ligado",
      DELAY_STARTUP_APP: "App de inicialização adiado",
      NETWORK_DIAGNOSTICS: "Rede verificada",
    },
  },
  notFound: {
    title: "404",
    message: "Página não encontrada",
    backHome: "Voltar ao início",
  },
} as const;
