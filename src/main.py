"""
BlazeScan - Ponto de entrada principal
"""

import sys
import os
import logging
import ctypes
import customtkinter as ctk
from typing import NoReturn
from src.update.updater_core import check_for_updates_and_prompt

should_exit = check_for_updates_and_prompt()
if should_exit:
    sys.exit(0)
# ==========================================================
# CONFIGURAÇÃO DE PATH (IMPORTANTE PARA PYINSTALLER)
# ==========================================================

def get_base_path() -> str:
    """
    Retorna o caminho base do projeto.
    Funciona tanto em desenvolvimento quanto no .exe (PyInstaller)
    """
    if getattr(sys, 'frozen', False):
        return sys._MEIPASS  # PyInstaller temp folder
    return os.path.dirname(os.path.abspath(__file__))


BASE_PATH = get_base_path()

if BASE_PATH not in sys.path:
    sys.path.insert(0, BASE_PATH)

# ==========================================================
# LOGGING
# ==========================================================

logging.basicConfig(
    level=logging.INFO,
    format='[%(levelname)s] %(message)s'
)

logger = logging.getLogger("BlazeScan")

# ==========================================================
# IMPORTS DA APP
# ==========================================================

try:
    from src.frontend.ui import App
except ImportError as e:
    logger.critical(f"Erro ao importar UI: {e}")
    logger.info("Verifique se as dependências estão instaladas e os paths estão corretos.")
    sys.exit(1)

# ==========================================================
# ADMIN
# ==========================================================

def is_admin() -> bool:
    try:
        return ctypes.windll.shell32.IsUserAnAdmin()
    except Exception:
        return False


def elevate_privileges():
    """
    Reinicia o programa como admin (Windows)
    """
    if sys.platform != "win32":
        return

    if is_admin():
        return

    try:
        script = sys.executable if getattr(sys, 'frozen', False) else os.path.abspath(sys.argv[0])

        params = " ".join([f'"{arg}"' for arg in sys.argv[1:]])

        ret = ctypes.windll.shell32.ShellExecuteW(
            None,
            "runas",
            sys.executable,
            f'"{script}" {params}',
            None,
            1
        )

        if ret <= 32:
            logger.error("Usuário recusou permissões de administrador.")
        else:
            sys.exit(0)

    except Exception as e:
        logger.error(f"Erro ao elevar privilégios: {e}")

# ==========================================================
# MAIN
# ==========================================================

def main() -> NoReturn:
    logger.info("=" * 40)
    logger.info("🔥 Iniciando BlazeScan")
    logger.info("=" * 40)

    # Admin
    elevate_privileges()

    if sys.platform != "win32":
        logger.warning("Sistema não é Windows. Algumas funções podem falhar.")

    if is_admin():
        logger.info("✔ Executando como Administrador")
    else:
        logger.warning("⚠ Executando sem privilégios de Admin")

    try:
        # Config global UI
        ctk.set_appearance_mode("System")
        ctk.set_default_color_theme("blue")

        app = App()
        app.mainloop()

    except KeyboardInterrupt:
        logger.info("Encerrado pelo usuário.")
        sys.exit(0)

    except Exception as e:
        logger.critical(f"Erro fatal: {e}")
        sys.exit(1)

    sys.exit(0)


if __name__ == "__main__":
    main()