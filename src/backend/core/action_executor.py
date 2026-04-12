from src.backend.core.cleanup import (
    cleanup_temp_files,
    cleanup_terminate_processes,
    cleanup_power_plan
)
from src.backend.ai.rules import fallback_rules

ALLOWED_ACTIONS = [
    "clear_temp_files",
    "close_background_apps",
    "high_performance",
    "no_action",
    "fallback"
]


def execute_action(action, system_data=None):
    if action not in ALLOWED_ACTIONS:
        return {"success": False, "message": "Ação não permitida"}

    if action == "clear_temp_files":
        return cleanup_temp_files()

    elif action == "close_background_apps":
        return cleanup_terminate_processes()

    elif action == "high_performance":
        return cleanup_power_plan({"energy_plan": "MAXIMUM_PERFORMANCE"})

    elif action == "fallback":
        return fallback_rules(system_data)

    return {"success": True, "message": "Nenhuma ação necessária"}