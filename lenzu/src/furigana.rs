//! Local furigana / romaji annotation via MeCab morphological analysis.
//!
//! Thin wrapper around the [`mecab_furigana_rs`] crate.  On any platform where
//! MeCab is installed (and a UTF-8 dictionary is discoverable), this module
//! annotates `TranslationResult` slices in-place with bracketed furigana and
//! Hepburn romaji.  Returns `false` when MeCab is unavailable.

use crate::client::TranslationResult;

/// Annotate results with furigana (and romaji unless `furigana_only`) using MeCab.
/// Returns `true` if any result was enriched.
pub fn annotate(results: &mut [TranslationResult], furigana_only: bool) -> bool {
    if mecab_furigana_rs::find_mecab_dict().is_none() {
        eprintln!("[furigana] no MeCab UTF-8 dictionary found — skipping annotation");
        return false;
    }

    let mut any = false;
    for result in results.iter_mut() {
        let text = result.original.trim();
        if text.is_empty() {
            continue;
        }

        let annotated = if furigana_only {
            mecab_furigana_rs::annotate_furigana_only(text)
        } else {
            mecab_furigana_rs::annotate(text)
        };

        if let Some(fr) = annotated {
            eprintln!(
                "[furigana] mecab — furigana={} romaji={} furigana_only={}",
                !fr.furigana.is_empty(),
                !fr.romaji.is_empty(),
                furigana_only,
            );
            result.furigana = Some(fr.furigana);
            if !furigana_only && !fr.romaji.is_empty() {
                result.romaji = Some(fr.romaji);
            }
            any = true;
        }
    }
    any
}

/// Compare LLM furigana against MeCab's dictionary-based readings.
/// Always logs timing and MATCH/MISMATCH (warnings on mismatch).
/// When `overwrite` is true, replaces the LLM furigana with MeCab's.
/// Returns `true` if any result was processed.
pub fn compare_and_maybe_overwrite(results: &mut [TranslationResult], overwrite: bool) -> bool {
    use std::time::Instant;

    if mecab_furigana_rs::find_mecab_dict().is_none() {
        eprintln!("[mecab-check] no MeCab UTF-8 dictionary found — skipping");
        return false;
    }

    let mut any = false;
    for result in results.iter_mut() {
        let text = result.original.trim();
        if text.is_empty() {
            continue;
        }

        let t0 = Instant::now();
        let mecab_result = mecab_furigana_rs::annotate(text);
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;

        if let Some(fr) = mecab_result {
            let llm_furigana = result.furigana.as_deref().unwrap_or("");

            let matched = llm_furigana == fr.furigana;
            if matched {
                eprintln!(
                    "[mecab-check] {:.1}ms MATCH «{}»",
                    elapsed_ms,
                    truncate_display(&fr.furigana, 60),
                );
            } else {
                eprintln!(
                    "[mecab-check] {:.1}ms WARNING MISMATCH (overwrite={})\n  llm:   «{}»\n  mecab: «{}»",
                    elapsed_ms,
                    overwrite,
                    truncate_display(llm_furigana, 80),
                    truncate_display(&fr.furigana, 80),
                );
            }

            if overwrite {
                result.furigana = Some(fr.furigana);
                if !fr.romaji.is_empty() {
                    result.romaji = Some(fr.romaji);
                }
            }
            any = true;
        } else {
            eprintln!(
                "[mecab-check] {:.1}ms MeCab returned nothing for «{}»",
                elapsed_ms,
                truncate_display(text, 40),
            );
        }
    }
    any
}

fn truncate_display(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max_chars).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use crate::client::TranslationResult;

    #[test]
    fn test_annotate_populates_fields() {
        let mut results = vec![TranslationResult {
            original: "食べる".to_string(),
            ..Default::default()
        }];
        if super::annotate(&mut results, false) {
            assert!(results[0].furigana.is_some(), "furigana should be set");
            assert!(results[0].romaji.is_some(), "romaji should be set");
        }
    }

    #[test]
    fn test_annotate_furigana_only_skips_romaji() {
        let mut results = vec![TranslationResult {
            original: "食べる".to_string(),
            ..Default::default()
        }];
        if super::annotate(&mut results, true) {
            assert!(results[0].furigana.is_some(), "furigana should be set");
            assert!(results[0].romaji.is_none(), "romaji should be None in furigana_only mode");
        }
    }

    #[test]
    fn test_annotate_empty_text_skipped() {
        let mut results = vec![TranslationResult {
            original: "".to_string(),
            ..Default::default()
        }];
        assert!(!super::annotate(&mut results, false));
        assert!(results[0].furigana.is_none());
    }
}
