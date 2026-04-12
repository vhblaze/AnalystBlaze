import logging
from typing import Dict, Any

from src.backend.utils.system import format_bytes
from src.backend.ai.rules import (
    classify_temp_size,
    classify_ram_usage,
    classify_disk_usage
)

logger = logging.getLogger('BlazeScan')


# ==========================================================
# GERADOR DE INSIGHTS
# ==========================================================


def generate_insights(system_data: Dict[str, Any]) -> Dict[str, Any]:
    insights = {}
    
    top = system_data.get("temp", {}).get("top_consumers", [])
    extra_message = ""
    

    # ======================================================
    # TEMP FILES
    # ======================================================


    total_temp_size = system_data.get("total_temp_size_bytes", 0)
    total_temp_readable = format_bytes(total_temp_size)

    temp_level = classify_temp_size(total_temp_size)
    if top:
        main = top[0]
        extra_message = (
            f" 🔥 Maior impacto: {main['name']} ocupa {main['size']}"
            f"({main.get('percent', '?')}% do total)."
        )
    if temp_level == "low":
        temp_message = "Quantidade baixa de arquivos temporários. Sistema está saudável."

    elif temp_level == "medium":
        temp_message = (
            f"Você tem {total_temp_readable} em arquivos temporários. "
            "Isso pode começar a impactar o desempenho em jogos mais pesados."
        )

    else: # high 
        temp_message = (
            f"⚠️ Você tem {total_temp_readable} em arquivos temporários. "
            "Isso pode causar travamentos (stutter), carregamento lento e queda de FPS."
        )

    temp_message += extra_message

    insights["temp_files"] = {
        "level": temp_level,
        "total_size_bytes": total_temp_size,
        "total_size_readable": total_temp_readable,
        "recommendation": "Execute a limpeza para liberar espaço e melhorar a estabilidade.",
        "message": temp_message
    }


    # ======================================================
    # RAM
    # ======================================================


    ram_percent = system_data.get("ram_percent")

    if ram_percent is not None:
        ram_level = classify_ram_usage(ram_percent)

        if ram_level == "low":
            ram_message = "Uso de RAM dentro do normal."

        elif ram_level == "medium":
            ram_message = "Uso moderado de RAM. Pode impactar jogos mais pesados."

        else:  # high
            ram_message = "⚠️ Uso alto de RAM pode causar travamentos e quedas de FPS."

        insights["ram"] = {
            "level": ram_level,
            "percent": ram_percent,
            "message": ram_message
        }


    # ======================================================
    # DISCO
    # ======================================================

    
    disk_percent = system_data.get("disk_percent")

    if disk_percent is not None:
        disk_level = classify_disk_usage(disk_percent)

        if disk_level == "low":
            disk_message = "Espaço em disco saudável."

        elif disk_level == "medium":
            disk_message = "Espaço em disco reduzido. Considere liberar espaço."

        else:  # high
            disk_message = "⚠️ Disco quase cheio pode causar lentidão e problemas de carregamento."

        insights["disk"] = {
            "level": disk_level,
            "percent": disk_percent,
            "message": disk_message
        }

    return insights