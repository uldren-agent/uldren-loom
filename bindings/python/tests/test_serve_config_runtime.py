import json

import uldrenai_loom


def test_serve_config_generated_wrappers_runtime_round_trip(tmp_path):
    for name in (
        "serve_listener_configure_json",
        "serve_listener_list_json",
        "serve_listener_set_enabled_json",
        "serve_listener_remove_json",
        "serve_web_route_list_json",
        "serve_web_route_set_json",
        "serve_web_route_remove_json",
    ):
        assert callable(getattr(uldrenai_loom, name))

    path = str(tmp_path / "serve.loom")
    uldrenai_loom.create_loom(path, "default", None, None)

    initial = json.loads(uldrenai_loom.serve_listener_list_json(path))
    assert initial["listeners"] == []

    listener = json.loads(
        uldrenai_loom.serve_listener_configure_json(
            path,
            json.dumps(
                {
                    "surface": "admin",
                    "selectors": [],
                    "bind": "127.0.0.1:18083",
                    "transport": "rest",
                    "enabled": True,
                    "auth_mode": "owner-or-passphrase",
                    "exposure": "read-write",
                    "audit_mode": "management-and-security",
                    "request_size_limit": 4096,
                    "idle_timeout_ms": 1000,
                    "session_timeout_ms": 2000,
                }
            ),
        )
    )
    assert listener["surface"] == "admin"
    assert listener["enabled"] is True
    assert listener["limits"]["request_size_limit"] == 4096

    disabled = json.loads(
        uldrenai_loom.serve_listener_set_enabled_json(path, listener["id"], False)
    )
    assert disabled["enabled"] is False
    enabled = json.loads(
        uldrenai_loom.serve_listener_set_enabled_json(path, listener["id"], True)
    )
    assert enabled["enabled"] is True

    listed = json.loads(uldrenai_loom.serve_listener_list_json(path))
    assert [item["id"] for item in listed["listeners"]] == [listener["id"]]

    workspace = uldrenai_loom.workspace_create(path, "web-root", "files")
    web_listener = json.loads(
        uldrenai_loom.serve_listener_configure_json(
            path,
            json.dumps(
                {
                    "surface": "web",
                    "selectors": ["web-root"],
                    "bind": "127.0.0.1:18084",
                    "transport": "rest",
                    "enabled": True,
                }
            ),
        )
    )
    assert web_listener["surface"] == "web"

    route = json.loads(
        uldrenai_loom.serve_web_route_set_json(
            path,
            json.dumps(
                {
                    "listener": web_listener["id"],
                    "route": "docs",
                    "prefix": "docs",
                    "workspace": "web-root",
                    "root": "/",
                }
            ),
        )
    )
    assert route["listener"] == web_listener["id"]
    assert route["default_workspace"] == workspace
    assert [item["route_id"] for item in route["routes"]] == ["docs"]
    assert route["routes"][0]["path_prefix"] == "/docs"

    route_list = json.loads(
        uldrenai_loom.serve_web_route_list_json(path, web_listener["id"])
    )
    assert route_list["routes"] == route["routes"]

    route_removed = json.loads(
        uldrenai_loom.serve_web_route_remove_json(path, web_listener["id"], "docs")
    )
    assert route_removed["routes"] == []

    removed = json.loads(uldrenai_loom.serve_listener_remove_json(path, listener["id"]))
    assert removed["id"] == listener["id"]
    reopened_list = json.loads(uldrenai_loom.serve_listener_list_json(path))
    assert [item["id"] for item in reopened_list["listeners"]] == [web_listener["id"]]
