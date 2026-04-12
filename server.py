from fastapi import FastAPI, Header, HTTPException
import logging
import os

app = FastAPI()
logger = logging.getLogger("BlazeScan")

# 🔐 SEGURANÇA
API_KEY = os.getenv("API_KEY", "dev-key")

# 🧠 "banco" fake
users = {
    "test-user": {
        "plan": "premium"
    }
}

VALID_ACTIONS = {
    "clear_temp_files",
    "close_background_apps",
    "optimize_power",
    "optimize_disk",
    "no_action"
}


@app.get("/")
def root():
    return {"status": "BlazeScan API online"}


# ==================================================
# 🔐 VALIDAÇÃO
# ==================================================
def validate_api_key(x_api_key: str):
    if x_api_key != API_KEY:
        raise HTTPException(status_code=401, detail="Unauthorized")


# ==================================================
# 🧠 DECISÃO
# ==================================================
@app.post("/decide")
def decide(data: dict, x_api_key: str = Header(None)):

    validate_api_key(x_api_key)

    user_id = data.get("user_id", "test-user")
    system = data.get("system_state", {})
    context = data.get("context", {})

    user = users.get(user_id, {"plan": "free"})
    plan = user["plan"]

    temp = system.get("total_temp_size_bytes", 0)
    ram = system.get("ram_percent", 0)

    # 🔒 FREE (usa fallback local)
    if plan == "free":
        return {"action": "no_action"}

    # 💎 PREMIUM
    action = "no_action"

    if temp > 1_500_000_000:
        action = "clear_temp_files"

    elif ram > 80:
        action = "close_background_apps"

    elif context.get("multitask"):
        action = "close_background_apps"

    # 🔒 segurança extra
    if action not in VALID_ACTIONS:
        action = "no_action"

    return {"action": action}


# ==================================================
# 📊 APRENDIZADO
# ==================================================
@app.post("/learn")
def learn(data: dict, x_api_key: str = Header(None)):

    validate_api_key(x_api_key)

    logger.info(f"📊 Aprendizado recebido: {data}")

    return {"status": "ok"}