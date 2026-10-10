from saambha.history import ChatHistory, Exchange, NewConversation


def test_nothing_said_yet(tmp_path):
    assert ChatHistory(tmp_path / "history.jsonl").recent(10) == []


def test_keeps_what_was_said_and_the_reply(tmp_path, clock):
    history = ChatHistory(tmp_path / "saambha" / "history.jsonl", clock)

    history.add("play music", "Playing Enter Hallownest")
    clock.advance(60)
    history.add('say "hi"\nthen stop', "Hi")

    assert history.recent(10) == [
        Exchange(clock.now - 60, "play music", "Playing Enter Hallownest"),
        Exchange(clock.now, 'say "hi"\nthen stop', "Hi"),
    ]


def test_recent_is_the_last_few(tmp_path, clock):
    history = ChatHistory(tmp_path / "history.jsonl", clock)
    for n in range(5):
        history.add(f"command {n}", f"reply {n}")

    assert [e.said for e in history.recent(2)] == ["command 3", "command 4"]


def test_new_conversation_is_marked(tmp_path, clock):
    history = ChatHistory(tmp_path / "history.jsonl", clock)
    history.add("play music", "Playing")
    clock.advance(60)

    history.start_conversation()

    assert history.recent(10) == [Exchange(clock.now - 60, "play music", "Playing"), NewConversation(clock.now)]


def test_new_conversation_is_marked_once(tmp_path, clock):
    history = ChatHistory(tmp_path / "history.jsonl", clock)
    history.add("play music", "Playing")

    history.start_conversation()
    history.start_conversation()

    assert history.recent(10)[1:] == [NewConversation(clock.now)]


def test_nothing_to_mark_a_new_conversation_after(tmp_path, clock):
    history = ChatHistory(tmp_path / "history.jsonl", clock)

    history.start_conversation()

    assert history.recent(10) == []
