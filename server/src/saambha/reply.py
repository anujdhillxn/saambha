"""The agent's reply in two parts: the summary, spoken aloud, and details, only shown in the chat."""
import re
from dataclasses import dataclass

DETAILS = re.compile(r"<details>(.*?)(?:</details>|$)", re.S)


@dataclass(frozen=True)
class Reply:
    summary: str
    details: str = ""


def parse(text: str) -> Reply:
    """[text] as the agent writes it: the details inside <details> tags, the summary around them. A
    model that leaves the tags out has written only a summary."""
    match = DETAILS.search(text)
    if not match:
        return Reply(text.strip())
    return Reply((text[:match.start()] + text[match.end():]).strip(), match.group(1).strip())
