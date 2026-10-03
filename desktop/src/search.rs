//! finding clips by what you'd say about them: "yesterday", "monday 3pm",
//! "recording", or a bit of the file name.

use chrono::NaiveDate;

use crate::format;
use crate::sync::Clip;

/// whether a clip answers to the query. every word has to show up somewhere
/// in what we'd say about the clip, so "mon 3pm" is monday's 3 o'clock clips
pub fn hit(clip: &Clip, query: &str, today: NaiveDate) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let words = words(clip, today);
    query.split_whitespace().all(|q| words.contains(q.to_lowercase().as_str()))
}

/// everything a clip answers to, lowercase: the time a few ways ("3:42 pm",
/// "3:42pm", "3pm", "15:42"), the day ("yesterday", "tuesday 29 september
/// 2026"), its kind and its name
fn words(clip: &Clip, today: NaiveDate) -> String {
    let t = format::local(clip.created);
    let kind = if clip.is_clip { "clip" } else { "recording" };
    format!(
        "{} {} {} {} {} {} {}",
        format::time(clip.created),
        t.format("%-I:%M%P %-I%P %H:%M"),
        format::day_on(clip.created, today),
        t.format("%A %-d %B %Y"),
        kind,
        clip.name,
        clip.name.replace(['_', '-'], " "),
    )
    .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};
    use std::path::PathBuf;

    // tuesday 29 september 2026, 3:42 pm, local time, seen from the wednesday
    fn clip(is_clip: bool) -> (Clip, NaiveDate) {
        let created = Local.with_ymd_and_hms(2026, 9, 29, 15, 42, 10).single().unwrap();
        let clip = Clip {
            key: "demo/1".into(),
            name: "2026-09-29_15-42-10.mp4".into(),
            is_clip,
            size: 1,
            created: created.timestamp(),
            duration_s: None,
            location: PathBuf::from("/nonexistent"),
        };
        (clip, NaiveDate::from_ymd_opt(2026, 9, 30).unwrap())
    }

    #[test]
    fn finds_by_time_day_kind_and_name() {
        let (c, today) = clip(true);
        for q in [
            "", "yesterday", "YESTERDAY", "tue", "tuesday", "29 sep", "sep 29", "september 2026", "3:42", "3:42 pm",
            "3:42pm", "3pm", "15:42", "pm", "clip", "09-29_15", "15 42 10", "yesterday 3:42", "tue pm clip",
        ] {
            assert!(hit(&c, q, today), "{q:?} should find it");
        }
        for q in ["today", "monday", "4:42", "am", "recording", "yesterday 4pm", "oct"] {
            assert!(!hit(&c, q, today), "{q:?} shouldn't find it");
        }
    }

    #[test]
    fn kind_and_relative_day_follow_the_clip() {
        let (c, today) = clip(false);
        assert!(hit(&c, "recording", today));
        assert!(!hit(&c, "clip", today));
        // the same clip, seen on its own day
        assert!(hit(&c, "today", NaiveDate::from_ymd_opt(2026, 9, 29).unwrap()));
        assert!(hit(&c, "tue 29 sep", NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()));
    }
}
