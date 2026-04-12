import os
import logging
import sys
from typing import Dict, Any, List

from src.backend.utils.system import (
    get_temp_paths,
    set_power_plan,
    optimize_disk,
    terminate_processes,
    format_bytes,
    clean_directory,
    OPT_PROCESSES_TO_KILL
)

logger = logging.getLogger('BlazeScan')


# ==========================================================
# RESULT PADRÃO
# ==========================================================

def result(success: bool, message: str, data: Any = None) -> Dict[str, Any]:
    return {
        "success": success,
        "message": message,
        "data": data if data is not None else {}
    }


# ==========================================================
# TEMP FILES
# ==========================================================

def cleanup_temp_files() -> Dict[str, Any]:
    logger.info("🧹 Limpando arquivos temporários...")

    total_cleaned_bytes = 0
    messages = []

    try:
        temp_paths_map = get_temp_paths()

        for name, path in temp_paths_map.items():
            if not path or not os.path.exists(path):
                continue

            try:
                cleaned_size = clean_directory(path)

                if cleaned_size:
                    total_cleaned_bytes += cleaned_size
                    messages.append(f"{name}: {format_bytes(cleaned_size)} liberados")

            except Exception as e:
                logger.error(f"Erro ao limpar {name}: {e}")
                messages.append(f"{name}: erro")

        return result(
            True,
            "Limpeza de temporários concluída",
            {
                "cleaned_bytes": total_cleaned_bytes,
                "details": messages
            }
        )

    except Exception as e:
        return result(False, f"Erro geral na limpeza de temp: {e}", {
            "cleaned_bytes": 0
        })


# ==========================================================
# PROCESSOS
# ==========================================================

def cleanup_terminate_processes() -> Dict[str, Any]:
    logger.info("🧠 Encerrando processos desnecessários...")

    try:
        success, terminated_list = terminate_processes(OPT_PROCESSES_TO_KILL)

        msg = (
            f"Processos encerrados: {', '.join(terminated_list)}"
            if terminated_list else
            "Nenhum processo encerrado"
        )

        return result(success, msg, {"terminated": terminated_list})

    except Exception as e:
        return result(False, f"Erro ao encerrar processos: {e}", {"terminated": []})


# ==========================================================
# ENERGIA
# ==========================================================

def cleanup_power_plan(settings: Dict[str, Any]) -> Dict[str, Any]:
    logger.info("⚡ Ajustando plano de energia...")

    try:
        plan_key = settings.get("energy_plan", "NONE")

        if plan_key == "NONE":
            return result(True, "Plano de energia não alterado")

        success, msg = set_power_plan(plan_key)

        if not success and plan_key == "MAXIMUM_PERFORMANCE":
            success, msg = set_power_plan("HIGH_PERFORMANCE")

        return result(success, msg)

    except Exception as e:
        return result(False, f"Erro ao ajustar energia: {e}")


# ==========================================================
# DISCO
# ==========================================================

def cleanup_disk_optimization(settings: Dict[str, Any]) -> Dict[str, Any]:
    logger.info("💽 Otimizando disco...")

    try:
        if not settings.get("optimize_disk", False):
            return result(True, "Otimização ignorada")

        if not sys.platform.startswith("win"):
            return result(False, "Apenas suportado no Windows")

        success, msg = optimize_disk("C")

        return result(success, msg)

    except Exception as e:
        return result(False, f"Erro ao otimizar disco: {e}")


# ==========================================================
# EXECUTOR DE AÇÃO (IA)
# ==========================================================

def execute_action(action: str, settings: Dict[str, Any]) -> Dict[str, Any]:
    """
    Executa apenas a ação escolhida pela IA
    """

    logger.info(f"🎯 Ação recebida da IA: {action}")

    if action == "clear_temp_files":
        return cleanup_temp_files()

    elif action == "close_background_apps":
        return cleanup_terminate_processes()

    elif action == "optimize_power":
        return cleanup_power_plan(settings)

    elif action == "optimize_disk":
        return cleanup_disk_optimization(settings)

    elif action == "no_action":
        return {
            "success": True,
            "skipped": True,
            "message": "Nenhuma ação necessária"
        }

    else:
        return {
            "success": False,
            "message": f"Ação desconhecida: {action}"
        }


# ==========================================================
# ORQUESTRADOR CLEANUP (FINAL)
# ==========================================================

def perform_cleanup(settings: Dict[str, Any], action: str) -> Dict[str, Any]:
    logger.info("=" * 50)
    logger.info("🔥 INICIANDO CLEANUP INTELIGENTE")
    logger.info("=" * 50)

    step = execute_action(action, settings)

    # segurança total contra None
    if not step:
        step = {
            "success": False,
            "message": "Ação não retornou resultado",
            "data": {}
        }

    data = step.get("data") or {}

    total_cleaned = data.get("cleaned_bytes", 0)
    formatted = format_bytes(total_cleaned)

    # impacto
    if total_cleaned > 2 * 1024**3:
        impact = "high"
    elif total_cleaned > 500 * 1024**2:
        impact = "medium"
    else:
        impact = "low"

    messages = {
        "high": "🔥 Grande limpeza realizada!",
        "medium": "⚡ Limpeza moderada aplicada.",
        "low": "✔ Sistema já estava otimizado."
    }

    summary = {
        "success": step.get("success", False),
        "action": action,
        "total_cleaned": formatted,
        "impact": impact,
        "message": messages[impact],
        "step": step
    }

    logger.info(f"FINALIZADO - {formatted} liberados")

    return summary