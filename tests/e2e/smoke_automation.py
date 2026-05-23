import os
import sys

from lib.api import DexterApi, DexterApiError


def main():
    api = DexterApi(os.environ.get("DEXTER_AUTOMATION_URL", "http://127.0.0.1:9877"))

    state = api.get("/state")
    assert state["ok"] is True
    assert "processing_stage" in state

    api.post("/panel/close")
    api.post("/wait", {"condition": "idle", "timeout_ms": 1000})

    api.post("/ptt/press")
    api.post("/wait", {"condition": "recording", "timeout_ms": 1000})
    api.post("/ptt/cancel")
    api.post("/wait", {"condition": "idle", "timeout_ms": 1000})

    try:
        api.post("/text", {"text": "   "})
    except DexterApiError:
        pass
    else:
        raise AssertionError("empty /text request should fail")

    errors = api.get("/console/errors")
    assert errors["ok"] is True

    events = api.get("/events")
    assert events["ok"] is True


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(exc, file=sys.stderr)
        raise
