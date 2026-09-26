"""Exercise the generated native bindings through UniFFI's actual C ABI."""

import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "dist" / "native"))
from app_core_bindings import AppCore, BindingError


def encode(value):
    return json.dumps(value).encode("utf-8")


def rejected(action):
    try:
        action()
    except BindingError:
        return
    raise AssertionError("invalid bridge call was accepted")


core, other = AppCore(), AppCore()
assert core.protocol_version() == 1
assert json.loads(core.view())["bootstrap"] == {"status": "idle"}
rejected(lambda: core.process_event(b"invalid JSON"))
requests = json.loads(core.process_event(encode({"type": "start"})))
assert json.loads(core.view())["bootstrap"] == {"status": "loading"}
request = next(item for item in requests if item["effect"]["type"] == "host_info")
render = next(item for item in requests if item["effect"]["type"] == "render")
success = encode({"Ok": {"name": "Native host", "version": "1.0"}})
rejected(lambda: core.handle_response(render["id"], success))
rejected(lambda: core.handle_response(request["id"], b"{}"))
effects = json.loads(core.handle_response(request["id"], success))
assert [effect["effect"]["type"] for effect in effects] == ["render"]
assert json.loads(core.view()) == {
    "initialized": True,
    "bootstrap": {"status": "ready", "value": {"name": "Native host", "version": "1.0"}},
}
rejected(lambda: core.handle_response(request["id"], success))
assert json.loads(other.view()) == {"initialized": False, "bootstrap": {"status": "idle"}}
print("native UniFFI: event -> effect -> result -> typed view PASS")
