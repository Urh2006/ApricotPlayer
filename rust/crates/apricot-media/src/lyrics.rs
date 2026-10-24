//! Display-ready lyrics and timed-line ranges for native text controls.

#[derive(Clone, Debug, PartialEq)]
pub struct TimedLyric {
    pub seconds: f64,
    pub start_utf16: usize,
    pub end_utf16: usize,
    pub rich_start: usize,
    pub rich_end: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LyricsDocument {
    pub text: String,
    pub timed_lines: Vec<TimedLyric>,
}

impl LyricsDocument {
    #[must_use]
    pub fn parse(raw: &str, source: &str) -> Self {
        let mut document = Self::default();
        if raw.trim().is_empty() {
            return document;
        }
        if !source.is_empty() {
            document.text.push_str(source);
            document.text.push_str("\r\n\r\n");
        }
        let mut offset = document.text.encode_utf16().count();
        let mut paragraph_count = document.text.matches("\r\n").count();
        for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let (time, text) =
                timed_line(line).map_or((None, line), |(time, text)| (Some(time), text));
            if text.is_empty() {
                continue;
            }
            let end = offset + text.encode_utf16().count();
            if let Some(seconds) = time {
                document.timed_lines.push(TimedLyric {
                    seconds,
                    start_utf16: offset,
                    end_utf16: end,
                    rich_start: offset - paragraph_count,
                    rich_end: end - paragraph_count,
                });
            }
            document.text.push_str(text);
            document.text.push_str("\r\n");
            offset = end + 2;
            paragraph_count += 1;
        }
        document.text.truncate(document.text.trim_end().len());
        document
    }

    #[must_use]
    pub fn active_line(&self, seconds: f64) -> Option<usize> {
        if !seconds.is_finite() {
            return None;
        }
        self.timed_lines
            .iter()
            .take_while(|line| line.seconds <= seconds)
            .count()
            .checked_sub(1)
    }
}

fn timed_line(line: &str) -> Option<(f64, &str)> {
    let (timestamp, text) = line.strip_prefix('[')?.split_once(']')?;
    let (minutes, seconds) = timestamp.split_once(':')?;
    let (whole, fraction) = seconds.split_once('.')?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(minutes) || !digits(whole) || !digits(fraction) {
        return None;
    }
    let time = minutes.parse::<f64>().ok()? * 60.0 + seconds.parse::<f64>().ok()?;
    time.is_finite().then_some((time, text.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_and_plain_lines_preserve_python_display_order() {
        let document = LyricsDocument::parse("[00:01.50] First\nPlain\n[00:03.00] Second", "Local");
        assert_eq!(document.text, "Local\r\n\r\nFirst\r\nPlain\r\nSecond");
        assert_eq!(document.active_line(1.0), None);
        assert_eq!(document.active_line(1.5), Some(0));
        assert_eq!(document.active_line(3.0), Some(1));
        assert_eq!(document.active_line(f64::NAN), None);
    }

    #[test]
    fn ranges_use_native_utf16_not_utf8_byte_offsets() {
        let document = LyricsDocument::parse("[00:00.00] \u{1f3b5}\n[00:02.00] \u{17e}", "");
        let utf16: Vec<_> = document.text.encode_utf16().collect();
        for (line, expected) in document.timed_lines.iter().zip(["\u{1f3b5}", "\u{17e}"]) {
            assert_eq!(
                String::from_utf16(&utf16[line.start_utf16..line.end_utf16]).unwrap(),
                expected
            );
        }
        let rich_text: Vec<_> = document.text.replace("\r\n", "\r").encode_utf16().collect();
        for (line, expected) in document.timed_lines.iter().zip(["\u{1f3b5}", "\u{17e}"]) {
            assert_eq!(
                String::from_utf16(&rich_text[line.rich_start..line.rich_end]).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn rich_ranges_account_for_header_and_untimed_paragraphs() {
        let document = LyricsDocument::parse("Plain\n[00:01.00] Timed", "Source");
        let rich: Vec<_> = document.text.replace("\r\n", "\r").encode_utf16().collect();
        let line = &document.timed_lines[0];
        assert_eq!(
            String::from_utf16(&rich[line.rich_start..line.rich_end]).unwrap(),
            "Timed"
        );
    }

    #[test]
    fn metadata_and_unsupported_tags_remain_readable_and_empty_input_has_no_header() {
        let document = LyricsDocument::parse("[ar:Artist]\n[00:03] Untimed\n[00:04.00] ", "");
        assert_eq!(document.text, "[ar:Artist]\r\n[00:03] Untimed");
        assert!(document.timed_lines.is_empty());
        assert_eq!(
            LyricsDocument::parse(" \n", "Online"),
            LyricsDocument::default()
        );
    }
}
