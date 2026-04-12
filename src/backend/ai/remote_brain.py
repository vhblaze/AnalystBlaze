import requests
import logging
from src.backend.config.settings import API_URL

logger = logging.getLogger("BlazeScan")

HEADERS = {
    "x-api-key": "blaze-secret"
}

VALID_ACTIONS = {
    "clear_temp_files",
    "close_background_apps",
    "optimize_power",
    "optimize_disk",
    "no_action"
}


# ==========================================================
# 🧠 FALLBACK LOCAL INTELIGENTE
# ==========================================================


def fallback_decision(system_data, context):
    mode = context.get("mode", "default")

    # 🔥 cenários protegidos
    if mode in ["gaming_media", "media_multitask"]:
        return "no_action"

    # 🎮 jogando
    if mode == "gaming":
        return "close_background_apps"

    # 🌐 navegador pesado e parado
    if context.get("heavy_browser") and not context.get("active_browser"):
        return "close_background_apps"

    # 🧹 muito lixo
    if system_data.get("total_temp_size_bytes", 0) > 500_000_000:
        return "clear_temp_files"

    # 💻 sistema pesado
    if context.get("high_ram"):
        return "close_background_apps"

    return "no_action"


# ==========================================================
# 🌐 DECISÃO PRINCIPAL (IA REMOTA + FALLBACK)
# ==========================================================


def get_best_action(system_data, context):
    try:
        response = requests.post(
            f"{API_URL}/decide",
            json={
                "system_state": system_data,
                "context": context
            },
            headers=HEADERS,
            timeout=3
        )

        response.raise_for_status()

        data = response.json()
        action = data.get("action")

        if action not in VALID_ACTIONS:
            logger.warning(f"Ação inválida da API: {action}")
            return fallback_decision(system_data, context)

        logger.info(f"🧠 IA remota decidiu: {action}")
        return action

    except Exception as e:
        logger.warning(f"🌐 IA offline → usando fallback inteligente: {e}")
        action = fallback_decision(system_data, context)
        logger.info(f"🧠 IA local decidiu: {action}")
        return action


# ==========================================================
# 📊 ENVIO DE RESULTADO (APRENDIZADO)
# ==========================================================


def send_result(action, before_score, after_score, system_data, context):
    try:
        requests.post(
            f"{API_URL}/learn",
            json={
                "user_id": "test-user",
                "action": action,
                "before_score": before_score,
                "after_score": after_score,
                "system_state": system_data,
                "context": context
            },
            headers=HEADERS,
            timeout=2
        )

    except Exception as e:
        logger.debug(f"Falha ao enviar aprendizado: {e}")