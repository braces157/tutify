"""Read-only YouTube Music RPC. Credentials arrive over stdin and stay in memory."""
import json
import sys


def run(request, factory=None, clients=None):
    import requests
    if factory is None:
        from ytmusicapi import YTMusic
        factory = YTMusic
    headers = request.get("headers")
    key = json.dumps(headers or {}, sort_keys=True)
    client = (clients or {}).get(key)
    if client is None:
        session = requests.Session()
        original_request = session.request

        def bounded_request(*args, **kwargs):
            kwargs.setdefault("timeout", (10, 25))
            return original_request(*args, **kwargs)

        session.request = bounded_request
        client = factory(auth=headers or None, requests_session=session, language="en", user=(headers or {}).get("x-goog-pageid") or None)
        if clients is not None:
            if len(clients) >= 2:
                clients.clear()
            clients[key] = client
    op = request["operation"]
    limit = max(1, min(int(request.get("limit", 51)), 100001))
    if op in ("account", "playlists", "liked") and not headers:
        return {"ok": False, "error": "authentication"}
    if op == "account":
        client.get_library_playlists(limit=1)
        return {"ok": True, "items": [], "complete": True}
    if op == "playlists":
        items = client.get_library_playlists(limit=limit)
        title = "YouTube Music playlists"
    elif op == "liked":
        result = client.get_liked_songs(limit=limit)
        items = result.get("tracks", [])
        title = "Liked Songs"
    elif op == "playlist":
        result = client.get_playlist(request["id"], limit=limit)
        items = result.get("tracks", [])
        title = result.get("title", "YouTube Music playlist")
    elif op == "album":
        result = client.get_album(request["id"])
        title = result.get("title", "Album")
        items = result.get("tracks", [])
        for item in items:
            item.setdefault("album", {"name": title, "id": request["id"]})
            if not item.get("artists"):
                item["artists"] = result.get("artists", [])
        return {"ok": True, "items": lean(items, op), "complete": True, "title": title}
    elif op == "artist":
        result = client.get_artist(request["id"])
        title = result.get("name", "Artist")
        songs = result.get("songs", {})
        if songs.get("browseId"):
            items = client.get_playlist(songs["browseId"], limit=limit).get("tracks", [])
        else:
            items = songs.get("results", [])
            for item in items:
                if not item.get("artists"):
                    item["artists"] = [{"name": title, "id": request["id"]}]
            return {"ok": True, "items": lean(items, op), "complete": True, "title": title}
    elif op == "search":
        items = client.search(request["query"], filter="songs", limit=limit)
        title = "YouTube Music search"
    elif op in ("recommendations", "track"):
        result = client.get_watch_playlist(videoId=request["id"], radio=(op == "recommendations"), limit=limit)
        items = result.get("tracks", [])
        title = "YouTube Music radio"
    else:
        return {"ok": False, "error": "request"}
    return {"ok": True, "items": lean(items[:limit], op), "complete": len(items) < limit, "title": title}


def lean(items, operation):
    """Do not return account feedback/tracking tokens or unnecessary payloads."""
    keys = ("playlistId", "title", "author") if operation == "playlists" else (
        "videoId", "title", "artists", "album", "duration_seconds", "duration", "length", "isAvailable"
    )
    return [{key: item[key] for key in keys if key in item} for item in items]


def response(raw, clients=None):
    try:
        request = json.loads(raw.decode("utf-8"))
        result = run(request, clients=clients)
    except ImportError:
        result = {"ok": False, "error": "setup"}
    except Exception as error:
        # Upstream exceptions may contain URLs, cookies or payloads. Emit a code only.
        message = str(error).lower()
        if "401" in message or "sign in" in message or "authentication" in message or "logged in" in message:
            code = "authentication"
        elif "403" in message or "private" in message or "permission" in message:
            code = "restricted"
        elif "404" in message or "not found" in message or "does not exist" in message:
            code = "missing"
        elif "429" in message or "too many requests" in message:
            code = "rate_limit"
        elif "timeout" in message or "connection" in message:
            code = "transport"
        else:
            code = "response"
        result = {"ok": False, "error": code}
    return result


def main():
    result = response(sys.stdin.buffer.read(131073))
    sys.stdout.buffer.write(json.dumps(result, ensure_ascii=False).encode("utf-8"))
    sys.stdout.buffer.flush()


def worker():
    clients = {}
    while True:
        line = sys.stdin.buffer.readline(131074)
        if not line:
            return
        if len(line) > 131073 or not line.endswith(b'\n'):
            return
        result = response(line, clients=clients)
        sys.stdout.buffer.write(json.dumps(result, ensure_ascii=False).encode("utf-8") + b'\n')
        sys.stdout.buffer.flush()


if __name__ == "__main__":
    if sys.argv[1:] == ['--worker']:
        worker()
    else:
        main()
