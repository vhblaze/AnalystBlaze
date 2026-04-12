import customtkinter as ctk
import threading
import sys
import subprocess
import os

from src.update.updater_core import is_update_available, download_update

ctk.set_appearance_mode("dark")
ctk.set_default_color_theme("blue")


# ================= PATH SEGURO =================
def get_asset_path(filename):
    if getattr(sys, 'frozen', False):
        return os.path.join(sys._MEIPASS, filename)
    return os.path.join(os.path.dirname(__file__), filename)


# ================= LAUNCHER =================
class Launcher(ctk.CTk):
    def __init__(self):
        super().__init__()

        # 🔥 ÍCONE SEGURO
        try:
            icon_path = get_asset_path("blazescan_logo.ico")
            if os.path.exists(icon_path):
                self.iconbitmap(icon_path)
        except Exception as e:
            print("Erro ao carregar ícone:", e)

        self.title("BlazeScan")
        self.geometry("420x300")
        self.resizable(False, False)

        # ================= CONTAINER =================
        self.container = ctk.CTkFrame(self, corner_radius=15)
        self.container.pack(expand=True, fill="both", padx=20, pady=20)

        # ================= TITLE =================
        self.title_label = ctk.CTkLabel(
            self.container,
            text="🔥 BlazeScan",
            font=ctk.CTkFont(size=22, weight="bold")
        )
        self.title_label.pack(pady=(15, 5))

        # ================= STATUS =================
        self.status = ctk.CTkLabel(
            self.container,
            text="Verificando atualizações...",
            font=ctk.CTkFont(size=12)
        )
        self.status.pack(pady=5)

        # ================= PROGRESS =================
        self.progress = ctk.CTkProgressBar(self.container, width=300)
        self.progress.set(0)
        self.progress.pack(pady=15)

        self.percent = ctk.CTkLabel(self.container, text="0%")
        self.percent.pack()

        # ================= BUTTON =================
        self.button = ctk.CTkButton(
            self.container,
            text="Verificar",
            command=self.check_update,
            width=200,
            height=40,
            corner_radius=10
        )
        self.button.pack(pady=20)

        # ================= DATA =================
        self.latest_version = None
        self.local_version = None

        self.check_update()

    # ================= CHECK =================
    def check_update(self):
        self.button.configure(state="disabled")

        def task():
            update, local, latest = is_update_available()

            self.local_version = local
            self.latest_version = latest

            if update:
                self.status.configure(text=f"Nova versão: {latest}")
                self.button.configure(
                    text="Atualizar",
                    command=self.start_update,
                    state="normal"
                )
            else:
                self.status.configure(text="✔ Atualizado")
                self.button.configure(
                    text="Abrir",
                    command=self.launch_app,
                    state="normal"
                )

        threading.Thread(target=task, daemon=True).start()

    # ================= UPDATE =================
    def start_update(self):
        self.button.configure(text="Atualizando...", state="disabled")

        def task():
            def progress_callback(p):
                self.progress.set(p / 100)
                self.percent.configure(text=f"{p}%")

            success, msg = download_update(
                latest_version=self.latest_version,
                current_exe=sys.executable,
                progress_callback=progress_callback
            )

            self.status.configure(text=msg)

            if success:
                self.after(1000, self.quit)
            else:
                self.button.configure(text="Tentar novamente", state="normal")

        threading.Thread(target=task, daemon=True).start()

    # ================= LAUNCH =================
    def launch_app(self):
        try:
            exe_path = sys.executable.replace("Launcher.exe", "BlazeScan.exe")
            subprocess.Popen([exe_path])
        except Exception as e:
            self.status.configure(text=f"Erro: {e}")

        self.quit()


# ================= RUN =================
if __name__ == "__main__":
    app = Launcher()
    app.mainloop()