import logging
from typing import Dict, Any

from src.backend.ai.rules import (
    classify_temp_size,
    classify_ram_usage,
    classify_disk_usage
)

logger = logging.getLogger('BlazeScan')


# ==========================================================
# PREDICTOR DE PERFORMANCE (LAG / STUTTER)
# ==========================================================

def predict_performance(system_data: Dict[str, Any]) -> Dict[str, Any]:
    risk_score = 0
    reasons = []


    # ======================================================
    # TEMP FILES
    # ======================================================


    temp_size = system_data.get("total_temp_size_bytes", 0)
    temp_level = classify_temp_size(temp_size)

    if temp_level == "high":
        risk_score += 30
        reasons.append("Muitos arquivos temporários podem causar stutter.")

    elif temp_level == "medium":
        risk_score += 15
        reasons.append("Arquivos temporários moderados.")


    # ======================================================
    # RAM
    # ======================================================


    ram_percent = system_data.get("ram_percent")

    if ram_percent:
        ram_level = classify_ram_usage(ram_percent)

        if ram_level == "high":
            risk_score += 40
            reasons.append("Uso alto de RAM pode causar travamentos.")

        elif ram_level == "medium":
            risk_score += 20
            reasons.append("Uso moderado de RAM.")


    # ======================================================
    # DISCO
    # ======================================================


    disk_percent = system_data.get("disk_percent")

    if disk_percent:
        disk_level = classify_disk_usage(disk_percent)

        if disk_level == "high":
            risk_score += 30
            reasons.append("Disco quase cheio afeta carregamento.")

        elif disk_level == "medium":
            risk_score += 15
            reasons.append("Espaço em disco reduzido.")


    # ======================================================
    # CLASSIFICAÇÃO FINAL
    # ======================================================


    if risk_score >= 70:
        level = "critical"
        message = "⚠️ Alto risco de lag e stutter em jogos."

    elif risk_score >= 40:
        level = "warning"
        message = "⚠️ Possível queda de FPS em jogos mais pesados."

    else:
        level = "safe"
        message = "Sistema estável para jogos."

    logger.info(f"Predictor: {level} ({risk_score})")

    return {
        "risk_score": risk_score,
        "level": level,
        "message": message,
        "reasons": reasons
    }