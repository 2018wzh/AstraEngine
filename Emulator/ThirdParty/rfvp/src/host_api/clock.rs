use super::{RfvpError, RfvpResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalendarTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub day_of_week: u8,
    pub hour: u8,
    pub minute: u8,
}

impl CalendarTime {
    pub fn validate(self) -> RfvpResult<()> {
        let leap = self.year % 4 == 0 && (self.year % 100 != 0 || self.year % 400 == 0);
        let days = match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return Err(RfvpError::InvalidData),
        };
        if self.year == 0
            || self.day == 0
            || self.day > days
            || self.day_of_week > 6
            || self.hour > 23
            || self.minute > 59
        {
            return Err(RfvpError::InvalidData);
        }
        Ok(())
    }
}

pub trait RfvpClock {
    fn ticks_us(&mut self) -> u64;

    fn local_calendar_time(&mut self) -> RfvpResult<CalendarTime> {
        Err(RfvpError::Unsupported)
    }
}
