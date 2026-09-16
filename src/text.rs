//! Post-processing for transcripts.

/// Clean up a transcript for pasting: collapse any pathological repetition
/// loops (a safety net — whisper rarely produces them) and normalize all
/// whitespace (including the newlines whisper inserts between segments) into a
/// single flowing line.
pub fn cleanup(text: &str) -> String {
    let collapsed = collapse_repetitions(text);
    collapsed.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Collapse a transcript that is the same word/phrase repeated over and over
/// into a single copy. Punctuation (and case) is ignored when comparing, and
/// trailing punctuation on the final token is tolerated.
pub fn collapse_repetitions(text: &str) -> String {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let n = tokens.len();
    if n < 4 {
        return text.to_string();
    }

    for period in 1..=n / 2 {
        if n % period != 0 {
            continue;
        }
        let k = n / period;
        // Single words collapse only after many repeats; multi-word phrases
        // collapse as soon as they repeat.
        let enough = if period == 1 { k >= 4 } else { k >= 2 };
        if !enough {
            continue;
        }

        let base = &tokens[..period];
        let periodic = (1..k).all(|b| blocks_equal(&tokens[b * period..(b + 1) * period], base));
        if periodic {
            return base.join(" ");
        }
    }

    text.to_string()
}

fn blocks_equal(a: &[&str], b: &[&str]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| norm(x) == norm(y))
}

fn norm(t: &str) -> String {
    t.trim_end_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::collapse_repetitions;

    #[test]
    fn collapses_many_repeated_single_words() {
        assert_eq!(collapse_repetitions("hello hello hello hello hello"), "hello");
        assert_eq!(
            collapse_repetitions("hello hello hello hello hello hello hello hello hello hello hello hello hello."),
            "hello"
        );
    }

    #[test]
    fn collapses_repeated_phrase() {
        let input = "The model is gated, accept the license. The model is gated, accept the license.";
        assert_eq!(
            collapse_repetitions(input),
            "The model is gated, accept the license."
        );
    }

    #[test]
    fn keeps_short_repeats() {
        assert_eq!(collapse_repetitions("no no"), "no no");
        assert_eq!(collapse_repetitions("no no no"), "no no no");
    }

    #[test]
    fn leaves_normal_text_untouched() {
        assert_eq!(
            collapse_repetitions("the quick brown fox jumps"),
            "the quick brown fox jumps"
        );
    }

    #[test]
    fn collapses_two_word_repeat() {
        assert_eq!(collapse_repetitions("one two one two"), "one two");
    }

    #[test]
    fn tolerates_trailing_punctuation_difference() {
        assert_eq!(collapse_repetitions("hello. hello. hello! hello?"), "hello.");
    }

    #[test]
    fn ignores_case() {
        assert_eq!(collapse_repetitions("No, no, no, no, no, no"), "No,");
    }
}
