import logging
from typing import Dict, Any

from src.backend.ai.rules import (
    classify_temp_size,
    classify_ram_usage,
    classify_disk_usage
)

logger = logging.getLogger('BlazeScan')

TEMP_WEIGHT = 30
RAM_WEIGHT = 40
DISK_WEIGHT = 30


def clamp(value, min_value=0, max_value=100):
    return max(min_value, min(value, max_value))


def calculate_score(system_data: Dict[str, Any]) -> Dict[str, Any]:
    score = 100
    risk = 0
    deductions = []


    # =========================
    # TEMP (INTELIGENTE)
    # =========================


    temp_size = system_data.get("total_temp_size_bytes", 0)
    temp_gb = temp_size / (1024 ** 3)

    temp_penalty = min(TEMP_WEIGHT, temp_gb * 10)  # escala contínua
    score -= temp_penalty
    risk += temp_penalty * 0.8

    if temp_gb > 0.5:
        deductions.append({
            "type": "temp_files",
            "impact": round(temp_penalty, 2),
            "reason": f"{temp_gb:.2f}GB de arquivos temporários."
        })


    # =========================
    # RAM (ESCALA CONTÍNUA)
    # =========================


    ram_percent = system_data.get("ram_percent", 0)

    ram_penalty = (ram_percent / 100) * RAM_WEIGHT
    score -= ram_penalty
    risk += ram_penalty * 1.2

    if ram_percent > 60:
        deductions.append({
            "type": "ram",
            "impact": round(ram_penalty, 2),
            "reason": f"Uso de RAM em {ram_percent}%."
        })


    # =========================
    # DISCO (ESCALA CONTÍNUA)
    # =========================


    disk_percent = system_data.get("disk_percent", 0)

    disk_penalty = (disk_percent / 100) * DISK_WEIGHT
    score -= disk_penalty
    risk += disk_penalty * 1.1

    if disk_percent > 70:
        deductions.append({
            "type": "disk",
            "impact": round(disk_penalty, 2),
            "reason": f"Disco em {disk_percent}%."
        })


    # =========================
    # SINERGIA (MODO STRESS)
    # =========================


    stress_factor = 1.0

    if ram_percent > 75 and disk_percent > 80:
        stress_factor += 0.25

    if temp_gb > 1 and ram_percent > 70:
        stress_factor += 0.20

    score *= (2 - stress_factor)  # reduz score em conjunto
    risk *= stress_factor


    # =========================
    # FINALIZAÇÃO
    # =========================


    score = clamp(score)
    risk = clamp(risk)

    # classificação
    if score >= 85:
        level = "excellent"
    elif score >= 70:
        level = "good"
    elif score >= 50:
        level = "warning"
    else:
        level = "critical"

    return {
        "score": round(score, 2),
        "level": level,
        "risk_score": round(risk, 2),
        "deductions": deductions
    }