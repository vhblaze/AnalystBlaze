import logging
from src.backend.core.orchestrator import run_blazescan

# Configura log no console
logging.basicConfig(level=logging.INFO)

if __name__ == "__main__":
    settings = {
        "energy_plan": "HIGH_PERFORMANCE",
        "optimize_disk": False
    }

    result = run_blazescan(settings)

    print("\n=== RESULTADO FINAL ===\n")
    print(result)