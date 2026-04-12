import os
import subprocess
import logging
from typing import List, Tuple, Optional, Dict
import shutil # Importação necessária para clean_directory

logger = logging.getLogger('BlazeScan')

# ====================================================================
# CONSTANTES E VARIÁVEIS DE CONFIGURAÇÃO
# ====================================================================

POWER_PLAN_GUIDS = {
    "MAXIMUM_PERFORMANCE": "e9a42b02-d5df-448d-aa00-03f147498387",
    "HIGH_PERFORMANCE": "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c",
    "BALANCED": "381b4222-f694-41f0-9685-ce5920ae7518",
    "POWER_SAVER": "a1841308-3541-4fab-bc81-f71556f20b4a"
}

OPT_PROCESSES_TO_KILL = [
    "epicgameslauncher.exe", 
    "steamwebhelper.exe", 
    "teams.exe",     
    "zoom.exe",      
    "onedrive.exe",  
    "dropbox.exe", 
    "utorrent.exe", 
    "qbittorrent.exe",
    "AnyDesk.exe",     
    "TeamViewer.exe",      
    "copilot.exe",
]

# ====================================================================
# FUNÇÕES DE UTILIDADE GERAL
# ====================================================================

def format_bytes(size_in_bytes: int) -> str:
    """Formata um número de bytes para uma string legível."""
    if size_in_bytes == 0:
        return "0 Bytes"
    units = ["Bytes", "KB", "MB", "GB", "TB", "PB"]
    i = 0
    size = float(size_in_bytes)
    while size >= 1024.0 and i < len(units) - 1:
        size /= 1024.0
        i += 1
    return f"{size:.2f} {units[i]}"

def execute_windows_command(command: List[str]) -> Tuple[bool, str]:
    """Executa comando com segurança (sem shell=True)."""

    try:
        result = subprocess.run(
            command,
            capture_output=True,
            text=True,
            shell=False  # 🔥 CORREÇÃO DE SEGURANÇA
        )

        stdout_output = result.stdout.strip()
        stderr_output = result.stderr.strip()

        if result.returncode == 0:
            return True, stdout_output

        # tratamento especial
        if "not found" in stderr_output.lower() or "não encontrado" in stderr_output.lower():
            return True, f"AVISO: {stderr_output}"

        return False, stderr_output or f"Código {result.returncode}"

    except Exception as e:
        return False, str(e)


# ====================================================================
# FUNÇÕES DE INTERAÇÃO COM O SISTEMA
# ====================================================================

def get_temp_paths() -> Dict[str, str]:
    user_temp = os.environ.get('TEMP')
    local_app_data = os.environ.get('LOCALAPPDATA')
    system_drive = os.environ.get('SystemDrive', 'C:')

    paths = {}

    if user_temp:
        paths['Temp Usuário'] = user_temp

    if local_app_data:
        local_temp = os.path.join(local_app_data, 'Temp')

        # Evita duplicação
        if local_temp != user_temp:
            paths['Temp Local'] = local_temp

        paths['Cache Edge'] = os.path.join(
            local_app_data, 'Microsoft', 'Edge', 'User Data', 'Default', 'Cache'
        )

        paths['Cache Chrome'] = os.path.join(
            local_app_data, 'Google', 'Chrome', 'User Data', 'Default', 'Cache'
        )

    if system_drive:
        paths['Temp Sistema'] = os.path.join(system_drive, 'Windows', 'Temp')
        paths['Prefetch'] = os.path.join(system_drive, 'Windows', 'Prefetch')

    return paths

def set_power_plan(plan_key: str) -> Tuple[bool, str]:
    """Define o plano de energia do Windows."""
    
    plan_upper = plan_key.upper()
    guid = POWER_PLAN_GUIDS.get(plan_upper)
    
    if not guid:
        logger.warning(f"Plano de energia '{plan_key}' desconhecido.")
        return False, f"Plano de energia '{plan_key}' desconhecido."
    
    # 1. Tentar ativar o plano
    command = ["powercfg", "/setactive", guid]
    success, output = execute_windows_command(command)
    
    # 2. SE FALHAR E FOR O 'DESEMPENHO MÁXIMO', TENTAR CRIÁ-LO (Lógica robusta)
    if not success and plan_upper == "MAXIMUM_PERFORMANCE":
        logger.warning("Falha ao ativar Desempenho Máximo. Tentando criá-lo primeiro...")
        
        # Comando para duplicar o plano HIGH_PERFORMANCE (8c5...) para o MAXIMUM_PERFORMANCE (e9a...)
        creation_command = ["powercfg", "/duplicate scheme", 
                            POWER_PLAN_GUIDS["HIGH_PERFORMANCE"], 
                            POWER_PLAN_GUIDS["MAXIMUM_PERFORMANCE"]]
        
        create_success, create_output = execute_windows_command(creation_command)
        
        if create_success:
            logger.info("Plano 'Desempenho Máximo' criado com sucesso. Tentando ativar novamente.")
            success, output = execute_windows_command(command)
            
            if success:
                logger.info(f"Plano de energia definido para {plan_key.replace('_', ' ').title()}.")
                return True, f"Plano de energia definido para {plan_key.replace('_', ' ').title()}."
        else:
            logger.error(f"Falha na criação do plano Desempenho Máximo. Output: {create_output}")
            return False, f"Falha na criação e ativação do plano de energia: {create_output}"

    # 3. RETORNO FINAL
    if success:
        logger.info(f"Plano de energia definido para {plan_key.replace('_', ' ').title()}.")
        return True, f"Plano de energia definido para {plan_key.replace('_', ' ').title()}."
    else:
        logger.error(f"Falha ao definir plano de energia: {output}")
        return False, f"Falha ao definir plano de energia: {output}"

def optimize_disk(drive_letter: str = "C") -> Tuple[bool, str]:
    """
    Executa a otimização (desfragmentação/TRIM) no disco especificado.
    Requer privilégios de Administrador.
    """
    if not drive_letter or not drive_letter.isalpha() or len(drive_letter) != 1:
        return False, "Letra da unidade inválida."

    drive_letter = drive_letter.upper()
    
    # Comando nativo do Windows: /O = Otimizar (Aplica TRIM em SSDs, desfragmenta HDDs)
    command = ["defrag", f"{drive_letter}:", "/O", "/V"] 
    
    logger.info(f"Iniciando otimização do disco {drive_letter}: com 'defrag /O'...")
    
    success, output = execute_windows_command(command)
    
    if success:
        if "completed" in output.lower() or "concluída" in output.lower() or "êxito" in output.lower():
            msg = f"Otimização do disco {drive_letter}: concluída com sucesso."
            logger.info(msg)
            return True, msg
        else:
            msg = f"Otimização do disco {drive_letter}: finalizada, mas verifique o log para detalhes. {output.strip().splitlines()[-1]}"
            logger.warning(msg)
            return True, msg
    else:
        msg = f"Falha na otimização do disco {drive_letter}:. Erro: {output}"
        logger.error(msg)
        return False, msg
    
def terminate_processes(processes: List[str], context: str = "default") -> Tuple[bool, List[str]]:
    """Encerra processos de forma inteligente (não fecha apps críticos)."""

    PROTECTED_PROCESSES = {
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "opera.exe",
        "spotify.exe",
        "discord.exe",
        "vlc.exe",
        "obs64.exe"
    }

    # 🔥 proteção dinâmica por contexto
    if context == "gaming":
        PROTECTED_PROCESSES.update({
            "steam.exe",
            "epicgameslauncher.exe"
        })

    if context == "media":
        PROTECTED_PROCESSES.update({
            "spotify.exe",
            "chrome.exe",
            "msedge.exe"
        })

    safe_processes = [
        p for p in processes
        if p.lower() not in PROTECTED_PROCESSES
    ]

    terminated_list = []
    overall_success = True

    logger.info(f"🧠 Encerrando {len(safe_processes)} processos (modo seguro)")

    for process_name in safe_processes:
        command = ["taskkill", "/F", "/IM", process_name]

        success, output = execute_windows_command(command)

        if success:
            if "AVISO:" not in output:
                terminated_list.append(process_name)
                logger.info(f"ENCERRADO: {process_name}")
        else:
            overall_success = False
            logger.warning(f"Falha ao encerrar '{process_name}': {output}")

    return overall_success, terminated_list

# ====================================================================
# FUNÇÕES DE LIMPEZA E CÁLCULO DE TAMANHO (CORREÇÃO DE ERRO ANTERIOR)
# ====================================================================

def get_dir_size(start_path: str) -> int:
    """Calcula o tamanho total de todos os arquivos em um diretório, em bytes."""
    total_size = 0
    if not os.path.exists(start_path):
        return 0
    try:
        for dirpath, dirnames, filenames in os.walk(start_path):
            for f in filenames:
                fp = os.path.join(dirpath, f)
                if not os.path.islink(fp):
                    try:
                        total_size += os.path.getsize(fp)
                    except OSError:
                        logger.debug(f"Permissão negada ou erro ao obter tamanho de: {fp}")
    except Exception as e:
        logger.debug(f"Erro ao calcular tamanho em {start_path}: {e}")
    return total_size

def clean_directory(path: str, skip_size: bool = False) -> int:
    
    """Remove todo o conteúdo de um diretório e retorna o tamanho liberado."""
    if not os.path.exists(path):
        return 0
        
    cleaned_size = 0 if skip_size else get_dir_size(path)
      
    # 2. Tenta remover TUDO
    try:
        # Percorre o conteúdo do diretório de destino
        for item in os.listdir(path):
            item_path = os.path.join(path, item)
            try:
                if os.path.isdir(item_path):
                    # Remove pastas recursivamente (ignora erros de permissão/arquivo em uso)
                    shutil.rmtree(item_path, ignore_errors=True)
                else:
                    # Remove arquivos (ignora erros de permissão/arquivo em uso)
                    os.remove(item_path)
            except Exception as sub_e:
                # Loga o arquivo/pasta específico que não pôde ser removido
                logger.debug(f" - Falha ao remover '{item_path}': {sub_e}")
        
    except Exception as e:
        logger.warning(f"Falha na limpeza de {path} (erro principal): {e}. Itens que estavam em uso podem ter permanecido.")
        
    # 3. Garante que o diretório base existe (importante para o TEMP, etc.)
    try:
        os.makedirs(path, exist_ok=True)
    except Exception as e:
        logger.error(f"Não foi possível recriar o diretório temporário {path}: {e}")
        
    return cleaned_size