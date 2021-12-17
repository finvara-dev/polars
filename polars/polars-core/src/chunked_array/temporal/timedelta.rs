use arrow::temporal_conversions::NANOSECONDS;
use crate::prelude::*;
use polars_time::Window;

#[cfg(feature = "dtype-datetime")]
impl DatetimeChunked {
    pub fn buckets(&self, every: Duration, offset: Duration) -> Self {
        let w = Window::new(every, every, offset);
        self.apply(|t| w.truncate(t)).into_date()
    }
}

#[cfg(feature = "dtype-date")]
impl DateChunked {
    pub fn buckets(&self, every: Duration, offset: Duration) -> Self {
        let w = Window::new(every, every, offset);
        self.apply(|t| {
            const NSECS_IN_DAY: i64 = NANOSECONDS * SECONDS_IN_DAY;
            (w.truncate( NSECS_IN_DAY * t as i64) / NSECS_IN_DAY) as i32
        }).into_date()
    }
}
