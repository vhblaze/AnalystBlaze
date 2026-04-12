import os
import subprocess
import hashlib
from datetime import datetime

PROJECT_NAME = "BlazeScan"
EXE_NAME = "BlazeScan.exe"
VERSION_FILE = "version/version.txt"
HASH_FILE = "version/hash.txt"

# =========================================
# CONFIG
# =========================================
MAIN_FILE = "src/main.py" # seu arquivo principal
DIST_PATH = "dist"

# =========================================
# HELPERS
# =========================================

def run(cmd):
    print(f"> {cmd}")
    result = subprocess.run(cmd, shell=True)
    if result.returncode != 0:
        raise Exception(f"Erro ao executar: {cmd}")

def get_version():
    if not os.path.exists(VERSION_FILE):
        return "v0.0.0"
    with open(VERSION_FILE, "r") as f:
        return f.read().strip()

def bump_version(version):
    v = version.replace("v", "").split(".")
    v[-1] = str(int(v[-1]) + 1)
    return "v" + ".".join(v)

def calculate_sha256(file_path):
    sha256 = hashlib.sha256()
    with open(file_path, "rb") as f:
        for chunk in iter(lambda: f.read(8192), b""):
            sha256.update(chunk)
    return sha256.hexdigest()

# =========================================
# BUILD
# =========================================

def build():
    print("\n🔥 Gerando EXE...")
    run(f"python -m PyInstaller --onefile {MAIN_FILE}")
    exe_path = os.path.join(DIST_PATH, MAIN_FILE.replace(".py", ".exe"))

    if not os.path.exists(exe_path):
        raise Exception("EXE não encontrado após build!")

    final_path = os.path.join(DIST_PATH, EXE_NAME)
    os.rename(exe_path, final_path)

    return final_path

# =========================================
# VERSION + HASH
# =========================================

def update_version_and_hash(exe_path):
    print("\n🔢 Atualizando versão e hash...")

    current_version = get_version()
    new_version = bump_version(current_version)

    # salva versão
    with open(VERSION_FILE, "w") as f:
        f.write(new_version)

    # gera hash
    file_hash = calculate_sha256(exe_path)
    with open(HASH_FILE, "w") as f:
        f.write(file_hash)

    print(f"Nova versão: {new_version}")
    print(f"Hash: {file_hash}")

    return new_version

# =========================================
# GIT + RELEASE
# =========================================

def git_commit_and_push(version):
    print("\n📦 Commitando...")

    run("git add .")
    run(f'git commit -m "release {version}"')
    run("git push")

def create_github_release(version, exe_path):
    print("\n🚀 Criando release no GitHub...")

    run(f'gh release create {version} "{exe_path}" --title "{version}" --notes "Release automática {version}"')

# =========================================
# MAIN
# =========================================

def main():
    try:
        exe_path = build()
        version = update_version_and_hash(exe_path)
        git_commit_and_push(version)
        create_github_release(version, exe_path)

        print("\n✅ RELEASE FINALIZADA COM SUCESSO!")

    except Exception as e:
        print(f"\n❌ ERRO: {e}")

if __name__ == "__main__":
    main()