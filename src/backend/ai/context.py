import psutil
import logging

logger = logging.getLogger("BlazeScan")

GAME_KEYWORDS = ["valorant", "cs2", "fortnite", "league", "dota", "minecraft"]
BROWSER_KEYWORDS = ["chrome", "edge", "firefox", "opera", "brave"]
MEDIA_KEYWORDS = ["spotify", "vlc"]


def get_process_data():
    processes = []

    for proc in psutil.process_iter(attrs=["name", "cpu_percent", "memory_info"]):
        try:
            name = proc.info["name"]
            if not name:
                continue

            name = name.lower()
            cpu = proc.cpu_percent(interval=0.1)
            ram = proc.info["memory_info"].rss / (1024 * 1024)  # MB

            processes.append({
                "name": name,
                "cpu": cpu,
                "ram": ram
            })

        except Exception:
            continue

    return processes


def detect_category(processes, keywords):
    return [
        p for p in processes
        if any(k in p["name"] for k in keywords)
    ]


def detect_context():
    try:
        processes = get_process_data()

        games = detect_category(processes, GAME_KEYWORDS)
        browsers = detect_category(processes, BROWSER_KEYWORDS)
        media = detect_category(processes, MEDIA_KEYWORDS)

        # 🔥 ATIVOS (CPU REAL)
        active_processes = [p for p in processes if p["cpu"] > 1.5]

        active_games = [p for p in games if p["cpu"] > 1.5]
        active_browsers = [p for p in browsers if p["cpu"] > 1.5]
        active_media = [p for p in media if p["cpu"] > 1.5]

        # 🔥 HEAVY (RAM)
        heavy_browsers = [p for p in browsers if p["ram"] > 500]

        context = {
            # presença
            "has_game": len(games) > 0,
            "has_browser": len(browsers) > 0,
            "has_media": len(media) > 0,

            # atividade real
            "active_game": len(active_games) > 0,
            "active_browser": len(active_browsers) > 0,
            "active_media": len(active_media) > 0,

            # peso
            "heavy_browser": len(heavy_browsers) > 0,

            # multitarefa real
            "multitask": len(active_processes) > 5,

            # sistema
            "high_ram": psutil.virtual_memory().percent > 75,
            "disk_pressure": psutil.disk_usage("C:\\").percent > 85,

            # debug
            "process_count": len(processes)
        }

        # 🔥 CLASSIFICAÇÃO FINAL (NÍVEL ALTO)
        if context["active_game"] and context["active_media"]:
            context["mode"] = "gaming_media"

        elif context["active_game"] and context["active_browser"]:
            context["mode"] = "gaming_browser"

        elif context["active_media"] and context["active_browser"]:
            context["mode"] = "media_multitask"

        elif context["active_game"]:
            context["mode"] = "gaming"

        elif context["active_media"]:
            context["mode"] = "media"

        else:
            context["mode"] = "default"

        logger.info(f"🧠 Contexto detectado: {context['mode']}")

        return context

    except Exception as e:
        logger.error(f"Erro ao detectar contexto: {e}")

        return {
            "mode": "default",
            "has_game": False,
            "has_browser": False,
            "has_media": False,
            "active_game": False,
            "active_browser": False,
            "active_media": False,
            "heavy_browser": False,
            "multitask": False,
            "high_ram": False,
            "disk_pressure": False,
            "process_count": 0
        }