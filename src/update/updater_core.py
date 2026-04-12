import requests
import os
import logging
import tempfile
import subprocess
import sys
import hashlib
from typing import Tuple, Optional
from packaging.version import parse as parse_version

logger = logging.getLogger('BlazeScan')

# =========================================
# CONFIG
# =========================================

GITHUB_VERSION_URL = "https://raw.githubusercontent.com/vhblaze/BlazeScan/main/version/version.txt"
GITHUB_HASH_URL = "https://raw.githubusercontent.com/vhblaze/BlazeScan/main/version/hash.txt"
GITHUB_RELEASE_DOWNLOAD_URL = "https://github.com/vhblaze/BlazeScan/releases/download/{version}/BlazeScan.exe"

EXECUTABLE_NAME = "BlazeScan.exe"
VERSION_FILE_REL_PATH = os.path.join("version", "version.txt")

# =========================================
# PATHS
# =========================================

def get_project_root() -> str:
    if getattr(sys, 'frozen', False):
        return os.path.dirname(sys.executable)
    return os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# =========================================
# VERSION
# =========================================

def get_local_version() -> Optional[str]:
    version_file = os.path.join(get_project_root(), VERSION_FILE_REL_PATH)

    try:
        with open(version_file, 'r', encoding='utf-8') as f:
            return f.read().strip()
    except Exception:
        return None


def get_latest_version() -> Optional[str]:
    try:
        response = requests.get(GITHUB_VERSION_URL, timeout=10)
        if response.status_code == 200:
            return response.text.strip()
    except Exception as e:
        logger.error(f"Erro ao buscar versão: {e}")

    return None


def is_update_available() -> Tuple[bool, Optional[str], Optional[str]]:
    local = get_local_version()
    latest = get_latest_version()

    if not local or not latest:
        return False, local, latest

    try:
        return parse_version(latest) > parse_version(local), local, latest
    except:
        return latest > local, local, latest

# =========================================
# HASH
# =========================================

def calculate_sha256(file_path: str) -> str:
    sha256 = hashlib.sha256()
    with open(file_path, "rb") as f:
        for chunk in iter(lambda: f.read(8192), b""):
            sha256.update(chunk)
    return sha256.hexdigest()


def get_expected_hash() -> Optional[str]:
    try:
        response = requests.get(GITHUB_HASH_URL, timeout=10)
        if response.status_code == 200:
            return response.text.strip()
    except Exception as e:
        logger.error(f"Erro ao buscar hash: {e}")

    return None

# =========================================
# REPLACEMENT SCRIPT
# =========================================

def launch_replacement_script(new_exe_path: str, old_exe_path: str) -> Tuple[bool, str]:
    old_exe_dir = os.path.dirname(old_exe_path)

    script_content = f"""
@echo off
echo Atualizando BlazeScan...

taskkill /F /IM "{EXECUTABLE_NAME}" > NUL 2>&1
timeout /t 2 /nobreak > NUL

move /Y "{new_exe_path}" "{old_exe_path}"

start "" "{old_exe_path}"

del "%~f0"
exit
"""

    bat_path = os.path.join(tempfile.gettempdir(), "update_blazescan.bat")

    try:
        with open(bat_path, "w") as f:
            f.write(script_content)

        subprocess.Popen(
            ["cmd", "/c", bat_path],
            close_fds=True,
            cwd=old_exe_dir
        )

        return True, "Atualização aplicada com sucesso."

    except Exception as e:
        logger.error(f"Erro no script de update: {e}")
        return False, str(e)

# =========================================
# DOWNLOAD COM PROGRESSO
# =========================================

def download_update(latest_version: str, local_executable_path: str) -> Tuple[bool, str]:
    url = GITHUB_RELEASE_DOWNLOAD_URL.format(version=latest_version)
    temp_file = os.path.join(tempfile.gettempdir(), f"{EXECUTABLE_NAME}.new")

    try:
        with requests.get(url, stream=True, timeout=60) as r:
            r.raise_for_status()

            total = int(r.headers.get('content-length', 0))
            downloaded = 0

            with open(temp_file, 'wb') as f:
                for chunk in r.iter_content(8192):
                    if chunk:
                        f.write(chunk)
                        downloaded += len(chunk)

                        if total > 0:
                            percent = int(downloaded * 100 / total)
                            bar = "█" * (percent // 3) + "░" * (33 - percent // 3)
                            sys.stdout.write(f"\r[{bar}] {percent}%")
                            sys.stdout.flush()

        print("\nDownload concluído.")

        # 🔐 valida hash
        expected = get_expected_hash()
        if not expected:
            return False, "Hash não encontrado"

        current_hash = calculate_sha256(temp_file)

        if current_hash != expected:
            os.remove(temp_file)
            return False, "Arquivo corrompido!"

        return launch_replacement_script(temp_file, local_executable_path)

    except Exception as e:
        logger.error(f"Erro no download: {e}")
        return False, str(e)

# =========================================
# MAIN UPDATE FLOW
# =========================================

def check_for_updates_and_prompt() -> bool:
    update_available, local, latest = is_update_available()

    if not update_available:
        logger.info("Sistema atualizado.")
        return False

    print("\n" + "=" * 50)
    print(f"🔥 Nova versão disponível: {latest}")
    print(f"Versão atual: {local}")

    if not getattr(sys, 'frozen', False):
        print("Modo dev - update ignorado")
        return False

    exe_path = sys.executable

    try:
        user = input("Atualizar agora? (S/n): ").lower().strip()
    except:
        user = "n"

    if user in ["", "s", "sim"]:
        success, msg = download_update(latest, exe_path)
        print(msg)

        if success:
            return True

    return False