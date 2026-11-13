//! What the chat panel lists: the current conversation as rows of times, commands and replies.

use gtk::glib::DateTime;

use crate::server::history::Entry;

const RUN_GAP_S: f64 = 600.0; // exchanges this close together share one time

#[derive(Debug, PartialEq)]
pub enum Row {
    When(String),
    Said(String),
    Reply(String),
    Details(String),
}

/// When something was said, in [now]'s time zone: the time today, else the date too.
pub fn when(time: f64, now: &DateTime) -> String {
    let then = DateTime::from_unix_utc(time as i64)
        .and_then(|t| t.to_timezone(&now.timezone()))
        .expect("a time the server wrote");
    let format = if then.ymd() == now.ymd() { "%H:%M" } else { "%b %-d, %H:%M" };
    then.format(format).expect("a valid format").into()
}

/// The rows of the exchanges since a new conversation was last started: a time starts each run.
pub fn rows(entries: &[Entry], now: &DateTime) -> Vec<Row> {
    let start = entries.iter().rposition(|e| e.new_conversation).map_or(0, |i| i + 1);
    let mut rows = Vec::new();
    let mut last: Option<f64> = None;
    for entry in &entries[start..] {
        if last.is_none_or(|last| entry.time - last >= RUN_GAP_S) {
            rows.push(Row::When(when(entry.time, now)));
        }
        rows.push(Row::Said(entry.said.clone()));
        rows.push(Row::Reply(entry.reply.clone()));
        if !entry.details.is_empty() {
            rows.push(Row::Details(entry.details.clone()));
        }
        last = Some(entry.time);
    }
    rows
}

#[cfg(test)]
mod tests {
    use gtk::glib::TimeZone;

    use super::*;

    fn now() -> DateTime {
        DateTime::new(&TimeZone::utc(), 2026, 10, 10, 16, 5, 0.0).unwrap()
    }

    fn at(hour: i32, minute: i32, day: i32) -> f64 {
        DateTime::from_utc(2026, 10, day, hour, minute, 0.0).unwrap().to_unix() as f64
    }

    fn exchange(time: f64, said: &str, reply: &str) -> Entry {
        Entry::exchange(time, said, reply, "")
    }

    fn new_conversation(time: f64) -> Entry {
        Entry::new_conversation(time)
    }

    #[test]
    fn today_shows_the_time() {
        assert_eq!(when(at(15, 5, 10), &now()), "15:05");
    }

    #[test]
    fn earlier_days_show_the_date_too() {
        assert_eq!(when(at(16, 5, 9), &now()), "Oct 9, 16:05");
    }

    #[test]
    fn a_time_starts_each_run_of_exchanges() {
        let entries = [
            exchange(at(15, 5, 10), "play music", "Playing"),
            exchange(at(15, 6, 10), "louder", "Done"),
            exchange(at(16, 4, 10), "what time is it", "4:04"),
        ];

        assert_eq!(rows(&entries, &now()), [
            Row::When("15:05".into()), Row::Said("play music".into()), Row::Reply("Playing".into()),
            Row::Said("louder".into()), Row::Reply("Done".into()),
            Row::When("16:04".into()), Row::Said("what time is it".into()), Row::Reply("4:04".into()),
        ]);
    }

    #[test]
    fn details_follow_their_reply() {
        let entries = [Entry { details: "Merge adds a commit.".into(), ..exchange(at(15, 5, 10), "merge or rebase", "Merge keeps both.") }];

        assert_eq!(rows(&entries, &now()), [
            Row::When("15:05".into()), Row::Said("merge or rebase".into()), Row::Reply("Merge keeps both.".into()),
            Row::Details("Merge adds a commit.".into()),
        ]);
    }

    #[test]
    fn only_the_conversation_since_a_new_one_was_last_started() {
        let entries = [
            exchange(at(15, 1, 10), "hello", "Hi"),
            new_conversation(at(15, 2, 10)),
            exchange(at(15, 5, 10), "play music", "Playing"),
            new_conversation(at(15, 6, 10)),
            exchange(at(15, 7, 10), "louder", "Done"),
        ];

        assert_eq!(rows(&entries, &now()), [Row::When("15:07".into()), Row::Said("louder".into()), Row::Reply("Done".into())]);
    }

    #[test]
    fn nothing_yet_in_a_new_conversation() {
        let entries = [exchange(at(15, 5, 10), "hi", "Hi"), new_conversation(at(15, 6, 10))];

        assert_eq!(rows(&entries, &now()), []);
    }

    #[test]
    fn no_rows_yet() {
        assert_eq!(rows(&[], &now()), []);
    }
}
