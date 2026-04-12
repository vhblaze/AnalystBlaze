import logging
import os
from typing import Dict, Any

import psutil

from src.backend.utils.system import get_temp_paths, format_bytes

logger = logging.getLogger('BlazeScan')

# ==========================================================
# UTIL: TAMANHO DE DIRETÓRIO
# ==========================================================

def get_directory_size(path: str) -> int:
    total_size = 0

    for root, dirs, files in os.walk(path):
        for file in files:
            try:
                total_size += os.path.getsize(os.path.join(root, file))
            except Exception:
                pass

    return total_size


# ==========================================================
# ANALISADORES ESPECÍFICOS
# ==========================================================

def analyze_temp_files() -> Dict[str, Any]:
    temp_paths_map = get_temp_paths()
    temp_data = {}
    total_size = 0
    seen_paths = set()
    for name, path in temp_paths_map.items():

        normalized_path = os.path.normpath(path).lower()

        if normalized_path in seen_paths:
            logger.warning(f"Caminho duplicado para {name}: {path} (ignorado)")
            continue

        seen_paths.add(normalized_path)

        if os.path.exists(path):
            try:
                size = get_directory_size(path)
                total_size += size

                temp_data[name] = {
                    "path": path,
                    "size_bytes": size,
                    "size_readable": format_bytes(size)
                }

            except Exception as e:
                logger.error(f"Erro ao analisar {name}: {e}")
                temp_data[name] = {
                    "path": path,
                    "error": str(e)
                }
        else:
            temp_data[name] = {
                "path": path,
                "error": "Caminho não encontrado"
            }
    top_files = sorted(
    temp_data.items(),
    key=lambda x: x[1].get("size_bytes", 0),
    reverse=True
)[:3]
    return {
        "files": temp_data,
        "total_bytes": total_size,
        "total_readable": format_bytes(total_size),
        "top_consumers": [
            {
                "name": name,
                "size": data.get("size_readable", "N/A"),
            }
            
            for name, data in top_files
        ]
        
    }


def analyze_ram() -> Dict[str, Any]:
    ram = psutil.virtual_memory()

    return {
        "percent": ram.percent,
        "available_gb": round(ram.available / (1024**3), 2),
        "total_gb": round(ram.total / (1024**3), 2)
    }


def analyze_disk() -> Dict[str, Any]:
    disk = psutil.disk_usage("C:\\")

    return {
        "percent": round((disk.used / disk.total) * 100, 2),
        "free_gb": round(disk.free / (1024**3), 2),
        "total_gb": round(disk.total / (1024**3), 2)
    }


# ==========================================================
# ANALISADOR PRINCIPAL
# ==========================================================

def collect_system_data() -> Dict[str, Any]:
    logger.info("📊 Coletando dados do sistema...")

    temp = analyze_temp_files()
    ram = analyze_ram()
    disk = analyze_disk()

    return {
        "temp": temp,
        "ram": ram,
        "disk": disk,

        # 🔥 BACKWARD COMPATIBILITY (pra não quebrar seu sistema atual)
        "total_temp_size_bytes": temp["total_bytes"],
        "ram_percent": ram["percent"],
        "disk_percent": disk["percent"]
    }