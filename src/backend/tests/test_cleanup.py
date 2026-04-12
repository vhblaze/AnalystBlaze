from src.backend.core.cleanup import perform_cleanup

settings = {
    "energy_plan": "NONE",
    "optimize_disk": False
}

result = perform_cleanup(settings)

print(result)