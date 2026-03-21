#!/usr/bin/env python3
"""Play a song on Spotify by search query.

Usage: spotify_play.py <search query>
Requires: SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET env vars
"""

import os
import sys
import logging
import urllib.request
import urllib.parse
import json
import base64

log = logging.getLogger("spotify-search")


def get_token(client_id: str, client_secret: str) -> str:
    credentials = base64.b64encode(f"{client_id}:{client_secret}".encode()).decode()
    data = urllib.parse.urlencode({"grant_type": "client_credentials"}).encode()
    req = urllib.request.Request(
        "https://accounts.spotify.com/api/token",
        data=data,
        headers={"Authorization": f"Basic {credentials}"},
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read())["access_token"]


def search_track(token: str, query: str) -> tuple[str, str, str]:
    """Returns (track_id, track_name, artist_name)"""
    params = urllib.parse.urlencode({"q": query, "type": "track", "limit": 1})
    req = urllib.request.Request(
        f"https://api.spotify.com/v1/search?{params}",
        headers={"Authorization": f"Bearer {token}"},
    )
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read())
    items = data["tracks"]["items"]
    if not items:
        raise RuntimeError(f"No tracks found for: {query}")
    track = items[0]
    return track["id"], track["name"], track["artists"][0]["name"]


def main():
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    if len(sys.argv) < 2:
        log.error("Usage: spotify_play.py <search query>")
        sys.exit(1)

    client_id = os.environ.get("SPOTIFY_CLIENT_ID")
    client_secret = os.environ.get("SPOTIFY_CLIENT_SECRET")
    if not client_id or not client_secret:
        log.error("SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET must be set")
        sys.exit(1)

    query = " ".join(sys.argv[1:])
    token = get_token(client_id, client_secret)
    track_id, name, artist = search_track(token, query)

    print(f"spotify:track:{track_id} | {name} by {artist}")


if __name__ == "__main__":
    main()
