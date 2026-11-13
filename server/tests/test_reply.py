from saambha.reply import Reply, parse


def test_a_summary_alone():
    assert parse("  Done, Starboy's playing. ") == Reply("Done, Starboy's playing.")


def test_the_details_come_after_the_summary():
    text = "Merge keeps both histories, rebase rewrites yours. More in the details.\n<details>\nMerge adds a commit.\n\nRebase replays.\n</details>"

    assert parse(text) == Reply("Merge keeps both histories, rebase rewrites yours. More in the details.",
                                "Merge adds a commit.\n\nRebase replays.")


def test_details_left_open_run_to_the_end():
    assert parse("Done.\n<details>It took two tries.") == Reply("Done.", "It took two tries.")


def test_the_summary_is_whatever_is_around_the_details():
    assert parse("<details>The long version.</details>\nThe short one.") == Reply("The short one.", "The long version.")
