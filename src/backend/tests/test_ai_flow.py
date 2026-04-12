from src.backend.core.orchestrator import run_blazescan

def main():
    settings = {
        "energy_plan": "HIGH_PERFORMANCE",
        "optimize_disk": False
    }

    result = run_blazescan(settings)

    print("\n" + "="*50)
    print("🔥 RESULTADO FINAL")
    print("="*50)

    print(f"\nStatus: {result['status']}")
    print(f"Ação: {result['action']}")
    print(f"Mensagem: {result['message']}")
    print(f"Melhoria: {result['improvement']:.2f}")

    print("\n--- BEFORE ---")
    print(f"Score: {result['before']['score']}")

    print("\n--- AFTER ---")
    print(f"Score: {result['after']['score']}")

    print("\n--- CLEANUP ---")
    print(result['cleanup'])


if __name__ == "__main__":
    main()