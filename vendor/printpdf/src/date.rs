/// wasm32-unknown-unknown polyfill
///
/// BoxDoc-Fork: siehe `vendor/printpdf/BOXDOC-PATCH.md`. Der Original-Polyfill
/// von printpdf 0.7.0 ist unvollständig — `offset()` fehlt und `month()` gibt
/// `u32` zurück, wo `document_info.rs` ein `u8` erwartet. Dadurch ließ sich
/// printpdf für `wasm32-unknown-unknown` überhaupt nicht bauen. Alle Änderungen
/// stehen in `cfg(target_arch = "wasm32")`-Blöcken; der native Codepfad ist
/// unverändert.

#[cfg(all(feature = "js-sys", target_arch = "wasm32", target_os = "unknown"))]
pub use self::js_sys_date::OffsetDateTime;

#[cfg(not(feature = "js-sys"))]
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use self::unix_epoch_stub_date::OffsetDateTime;

#[cfg(not(any(target_arch = "wasm32", target_os = "unknown")))]
pub use time::OffsetDateTime;

#[cfg(all(feature = "js-sys", target_arch = "wasm32", target_os = "unknown"))]
mod js_sys_date {
    use js_sys::Date;
    #[derive(Debug, Clone)]
    pub struct OffsetDateTime(Date);
    impl OffsetDateTime {
        #[inline(always)]
        pub fn now_utc() -> Self {
            let date = Date::new_0();
            OffsetDateTime(date)
        }

        #[inline(always)]
        pub fn now() -> Self {
            let date = Date::new_0();
            OffsetDateTime(date)
        }

        #[inline(always)]
        pub fn format(&self, format: impl ToString) -> String {
            // TODO
            "".into()
        }

        /// Zeitzonen-Abstand zu UTC.
        ///
        /// BoxDoc-Patch: fehlte im Original, wodurch `document_info.rs`
        /// (`to_pdf_time_stamp_metadata`) für wasm32 nicht kompilierte.
        ///
        /// `Date::get_timezone_offset()` liefert die Minuten, die man zur
        /// **lokalen** Zeit addieren muss, um UTC zu erhalten — also mit
        /// umgekehrtem Vorzeichen zur PDF-Konvention (UTC+2 → `-120`).
        /// Deshalb hier negieren. Die übrigen Getter lesen ebenfalls die
        /// lokale Zeit (`get_full_year` & Co.), Zeit und Abstand passen also
        /// zusammen.
        #[inline(always)]
        pub fn offset(&self) -> time::UtcOffset {
            let minutes = -self.0.get_timezone_offset();
            let seconds = if minutes.is_finite() {
                (minutes * 60.0) as i32
            } else {
                0
            };
            time::UtcOffset::from_whole_seconds(seconds).unwrap_or(time::UtcOffset::UTC)
        }

        #[inline(always)]
        pub fn year(&self) -> u32 {
            self.0.get_full_year()
        }

        /// BoxDoc-Patch: gibt `u8` statt `u32` zurück. `document_info.rs`
        /// ruft `u8::from(date.month())` — und `u8: From<u32>` gibt es nicht.
        #[inline(always)]
        pub fn month(&self) -> u8 {
            (self.0.get_month() + 1u32) as u8
        }

        #[inline(always)]
        pub fn day(&self) -> u32 {
            self.0.get_date()
        }

        #[inline(always)]
        pub fn hour(&self) -> u32 {
            self.0.get_hours()
        }

        #[inline(always)]
        pub fn minute(&self) -> u32 {
            self.0.get_minutes()
        }

        #[inline(always)]
        pub fn second(&self) -> u32 {
            self.0.get_seconds()
        }
    }
}

#[cfg(not(feature = "js-sys"))]
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod unix_epoch_stub_date {
    #[derive(Debug, Clone)]
    pub struct OffsetDateTime;
    impl OffsetDateTime {
        #[inline(always)]
        pub fn now_utc() -> Self {
            OffsetDateTime
        }

        #[inline(always)]
        pub fn now() -> Self {
            OffsetDateTime
        }

        #[inline(always)]
        pub fn format(&self, format: impl ToString) -> String {
            // TODO
            "".into()
        }

        /// BoxDoc-Patch: siehe `js_sys_date::offset`.
        #[inline(always)]
        pub fn offset(&self) -> time::UtcOffset {
            time::UtcOffset::UTC
        }

        #[inline(always)]
        pub fn year(&self) -> u32 {
            1970
        }

        /// BoxDoc-Patch: `u8` statt `u32` — siehe `js_sys_date::month`.
        #[inline(always)]
        pub fn month(&self) -> u8 {
            1
        }

        #[inline(always)]
        pub fn day(&self) -> u32 {
            1
        }

        #[inline(always)]
        pub fn hour(&self) -> u32 {
            0
        }

        #[inline(always)]
        pub fn minute(&self) -> u32 {
            0
        }

        #[inline(always)]
        pub fn second(&self) -> u32 {
            0
        }
    }
}
