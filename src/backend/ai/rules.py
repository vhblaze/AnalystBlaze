# ==========================================================
# REGRAS DE NEGÓCIO (IA)
# ==========================================================

# Limites de TEMP (em bytes)
TEMP_LOW = 500 * 1024**2      # 500MB
TEMP_MEDIUM = 2 * 1024**3     # 2GB

# RAM (%)
RAM_HIGH = 75
RAM_CRITICAL = 90

# DISCO (%)
DISK_HIGH = 75
DISK_CRITICAL = 90


# ==========================================================
# CLASSIFICAÇÕES
# ==========================================================


def fallback_rules(system_data):
    temp = system_data.get("total_temp_size_bytes", 0)

    if temp > 1_500_000_000:
        return {
            "success": True,
            "message": "Fallback: limpeza de temporários",
        }

    return {
        "success": True,
        "message": "Fallback: nenhuma ação"
    }

def classify_temp_size(size_bytes: int) -> str:
    if size_bytes < TEMP_LOW:
        return "low"
    elif size_bytes < TEMP_MEDIUM:
        return "medium"
    return "high"


def classify_ram_usage(ram_percent: float) -> str:
    if ram_percent < RAM_HIGH:
        return "low"
    elif ram_percent < RAM_CRITICAL:
        return "medium"
    return "high"


def classify_disk_usage(disk_percent: float) -> str:
    if disk_percent < DISK_HIGH:
        return "low"
    elif disk_percent < DISK_CRITICAL:
        return "medium"
    return "high"