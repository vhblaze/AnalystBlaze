import logging
from typing import Dict, Any

from src.backend.ai.analyzer import collect_system_data
from src.backend.ai.insights import generate_insights
from src.backend.ai.scorer import calculate_score
from src.backend.ai.predictor import predict_performance

from src.backend.ai.remote_brain import get_best_action, send_result
from src.backend.ai.context import detect_context
from src.backend.core.cleanup import perform_cleanup


logger = logging.getLogger('BlazeScan')


VALID_ACTIONS = {
    "clear_temp_files",
    "close_background_apps",
    "optimize_power",
    "optimize_disk",
    "no_action"
}


def run_blazescan(settings: Dict[str, Any]) -> Dict[str, Any]:
    logger.info("=" * 50)
    logger.info("🔥 INICIANDO BLAZESCAN ORCHESTRATOR")
    logger.info("=" * 50)

    # =========================================
    # 1. ANÁLISE ANTES
    # =========================================

    logger.info("📊 Coletando dados (ANTES)...")

    data_before = collect_system_data()
    insights_before = generate_insights(data_before)
    score_before_data = calculate_score(data_before)
    prediction_before = predict_performance(data_before)

    score_before = score_before_data.get("score", 0)

    # =========================================
    # 2. CONTEXTO + IA
    # =========================================

    context = detect_context()

    logger.info("🧠 Decidindo ação via IA...")
    action = get_best_action(data_before, context)

    # 🔒 VALIDAÇÃO DE SEGURANÇA
    if action not in VALID_ACTIONS:
        logger.warning(f"Ação inválida recebida: {action}")
        action = "no_action"

    logger.info(f"🎯 Ação escolhida: {action}")

    # =========================================
    # 3. EXECUÇÃO (NOVO CLEANUP)
    # =========================================

    logger.info("⚙️ Executando ação...")

    try:
        cleanup_result = perform_cleanup(settings, action)
    except Exception as e:
        logger.error(f"Erro ao executar ação: {e}")
        cleanup_result = {
            "success": False,
            "message": str(e),
            "data": {}
        }

    # =========================================
    # 4. ANÁLISE DEPOIS
    # =========================================

    logger.info("📊 Coletando dados (DEPOIS)...")

    data_after = collect_system_data()
    insights_after = generate_insights(data_after)
    score_after_data = calculate_score(data_after)
    prediction_after = predict_performance(data_after)

    score_after = score_after_data.get("score", 0)

    # =========================================
    # 5. MELHORIA + FEEDBACK IA
    # =========================================

    improvement = score_after - score_before

    try:
        send_result(
            action=action,
            before_score=score_before,
            after_score=score_after,
            system_data=data_after,
            context=context
        )
    except Exception as e:
        logger.debug(f"Falha ao enviar aprendizado: {e}")

    # =========================================
    # 6. STATUS FINAL
    # =========================================

    if score_after >= 85:
        status = "optimized"
    elif score_after >= 70:
        status = "good"
    else:
        status = "needs_attention"

    # =========================================
    # 7. MENSAGEM FINAL
    # =========================================

    if improvement > 20:
        final_message = "🚀 Grande melhoria no desempenho detectada!"
    elif improvement > 5:
        final_message = "⚡ Pequena melhoria aplicada."
    elif improvement > 0:
        final_message = "✔ Pequena otimização realizada."
    else:
        final_message = "✔ Sistema já estava bem otimizado."

    # =========================================
    # 8. RESULTADO FINAL
    # =========================================

    result = {
        "status": status,
        "message": final_message,
        "action": action,

        "before": {
            "score": score_before,
            "level": score_before_data.get("level"),
            "prediction": prediction_before,
            "insights": insights_before,
            "data": data_before,
            "details": score_before_data.get("deductions", [])
        },

        "after": {
            "score": score_after,
            "level": score_after_data.get("level"),
            "prediction": prediction_after,
            "insights": insights_after,
            "data": data_after,
            "details": score_after_data.get("deductions", [])
        },

        "cleanup": cleanup_result,
        "improvement": improvement
    }

    logger.info("✅ BLAZESCAN FINALIZADO")
    logger.info(f"Score: {score_before} → {score_after} ({improvement:+})")
    logger.info(f"Ação executada: {action}")

    return result