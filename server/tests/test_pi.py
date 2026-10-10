import threading

from conftest import PI_GUARD, SYSTEM_PROMPT


def arg(call, flag):
    return call["args"][call["args"].index(flag) + 1]


def test_returns_the_reply_and_streams_events(pi):
    events = []

    reply = pi.ask("play music", on_event=lambda kind, detail: events.append((kind, detail)))

    assert reply == "You said: play music"
    assert events == [("tool_use", "skill spotify"), ("text", "You said: play music")]


def test_other_tools_are_reported_by_name(pi, fakes):
    events = []

    pi.ask("what time is it", on_event=lambda kind, detail: events.append((kind, detail)))

    assert events == [("text", "You said: what time is it")]


def test_runs_in_the_workspace_with_prompt_skills_and_guard(pi, fakes, workspace, tmp_path):
    pi.ask("hi")

    [call] = fakes.pi_calls()
    assert call["cwd"] == str(workspace)
    assert arg(call, "--mode") == "json" and "--print" in call["args"]
    assert arg(call, "--model") == "anthropic/test-model"
    assert arg(call, "--append-system-prompt") == SYSTEM_PROMPT.read_text().strip()
    assert arg(call, "--skill") == str(workspace / "skills")
    assert arg(call, "--extension") == str(PI_GUARD)
    assert "--no-context-files" in call["args"]
    assert arg(call, "--session-dir") == str(tmp_path / "pi-sessions")
    assert call["args"][-2:] == ["--", "hi"]


def test_message_starting_with_a_dash_is_not_an_option(pi, fakes):
    assert pi.ask("--help me") == "You said: --help me"


def test_follow_up_continues_the_same_session(pi, fakes):
    pi.ask("first")
    pi.ask("second")

    first, second = fakes.pi_calls()
    assert arg(first, "--session-id") == arg(second, "--session-id") == "session-1"
    assert pi.has_session()


def test_reset_starts_a_new_session(pi, fakes):
    pi.ask("first")
    pi.reset()
    pi.ask("second")

    first, second = fakes.pi_calls()
    assert arg(first, "--session-id") == "session-1"
    assert arg(second, "--session-id") == "session-2"


def test_model_error_is_reported(pi, fakes):
    fakes.set("pi_mode", "error")

    assert pi.ask("hello") == "Sorry, something went wrong: Connection error."


def test_error_followed_by_a_successful_retry(pi, fakes):
    fakes.set("pi_mode", "retry")

    assert pi.ask("hello") == "You said: hello"


def test_not_logged_in(pi, fakes):
    fakes.set("pi_mode", "no-auth")

    assert pi.ask("hello") == "Sorry, something went wrong: No API key found for anthropic."


def test_hung_pi_is_killed_after_the_timeout(pi, fakes):
    pi.timeout = 1
    fakes.set("pi_mode", "hang")
    replies = []

    asking = threading.Thread(target=lambda: replies.append(pi.ask("hello")), daemon=True)
    asking.start()
    asking.join(timeout=5)

    assert replies == ["Sorry, that took too long. Please try again."]
