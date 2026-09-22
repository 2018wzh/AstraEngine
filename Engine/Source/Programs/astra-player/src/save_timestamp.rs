/// User-facing save time shared by desktop and native Player entry points.
/// If local timezone information is unavailable, UTC is explicitly labelled.
pub fn current_save_timestamp() -> String {
    match time::OffsetDateTime::now_local() {
        Ok(now) => format_timestamp(now, false),
        Err(_) => {
            tracing::warn!(
                event = "player.save.local_time_unavailable",
                "local timezone unavailable; save time is labelled UTC"
            );
            format_timestamp(time::OffsetDateTime::now_utc(), true)
        }
    }
}

fn format_timestamp(now: time::OffsetDateTime, utc: bool) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}{}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        if utc { " UTC" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_time_uses_the_local_calendar_day_and_labels_utc() {
        let utc = time::OffsetDateTime::from_unix_timestamp(82_800).unwrap();
        let local = utc.to_offset(time::UtcOffset::from_hms(8, 0, 0).unwrap());
        assert_eq!(format_timestamp(local, false), "1970-01-02 07:00");
        assert_eq!(format_timestamp(utc, true), "1970-01-01 23:00 UTC");
    }
}
