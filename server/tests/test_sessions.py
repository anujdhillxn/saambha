from saambha.agent.sessions import SessionStore


def test_session_round_trip(tmp_path, clock):
    store = SessionStore(tmp_path / "config" / "session_id", ttl=3600, clock=clock)

    assert store.load() is None
    store.save("sess-1")

    assert store.load() == "sess-1"


def test_session_expires_after_ttl(tmp_path, clock):
    store = SessionStore(tmp_path / "session_id", ttl=3600, clock=clock)
    store.save("sess-1")
    saved_at = store.path.stat().st_mtime

    clock.now = saved_at + 3599
    assert store.load() == "sess-1"
    clock.now = saved_at + 3601
    assert store.load() is None
    assert not store.path.exists()


def test_clear(tmp_path, clock):
    store = SessionStore(tmp_path / "session_id", ttl=3600, clock=clock)
    store.save("sess-1")

    store.clear()
    store.clear()  # nothing left to clear

    assert store.load() is None
