//! The agent's reply in two parts: the summary, spoken aloud, and details, only shown in the chat.

#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub summary: String,
    pub details: String,
}

/// [text] as the agent writes it: the details inside <details> tags, the summary around them. A model
/// that leaves the tags out has written only a summary.
pub fn parse(text: &str) -> Reply {
    let Some((before, rest)) = text.split_once("<details>") else {
        return Reply { summary: text.trim().to_string(), details: String::new() };
    };
    let (details, after) = rest.split_once("</details>").unwrap_or((rest, ""));
    Reply { summary: format!("{before}{after}").trim().to_string(), details: details.trim().to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(summary: &str, details: &str) -> Reply {
        Reply { summary: summary.into(), details: details.into() }
    }

    #[test]
    fn a_summary_alone() {
        assert_eq!(parse("  Done, Starboy's playing. "), reply("Done, Starboy's playing.", ""));
    }

    #[test]
    fn the_details_come_after_the_summary() {
        let text = "Merge keeps both, rebase rewrites. More in the details.\n<details>\nMerge adds a commit.\n\nRebase replays.\n</details>";

        assert_eq!(parse(text), reply("Merge keeps both, rebase rewrites. More in the details.", "Merge adds a commit.\n\nRebase replays."));
    }

    #[test]
    fn details_left_open_run_to_the_end() {
        assert_eq!(parse("Done.\n<details>It took two tries."), reply("Done.", "It took two tries."));
    }

    #[test]
    fn the_summary_is_whatever_is_around_the_details() {
        assert_eq!(parse("<details>The long version.</details>\nThe short one."), reply("The short one.", "The long version."));
    }
}
