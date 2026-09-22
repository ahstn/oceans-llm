use std::{ops::Range, sync::LazyLock};

use aho_corasick::AhoCorasick;
use regex::{Regex, RegexBuilder};

use super::{
    SecretRedactionConfig,
    rules::{RULES, Rule},
};

const REGEX_SIZE_LIMIT: usize = 32 * 1024 * 1024;

struct CompiledRule {
    rule: &'static Rule,
    regex: Regex,
}

struct Scanner {
    rules: Vec<CompiledRule>,
    keywords: AhoCorasick,
    /// Rule index for each keyword pattern in `keywords`.
    keyword_rules: Vec<usize>,
}

static SCANNER: LazyLock<Scanner> = LazyLock::new(Scanner::build);

impl Scanner {
    fn build() -> Self {
        let rules = RULES
            .iter()
            .map(|rule| CompiledRule {
                rule,
                regex: RegexBuilder::new(rule.pattern)
                    .size_limit(REGEX_SIZE_LIMIT)
                    .build()
                    .unwrap_or_else(|error| {
                        panic!("secret rule `{}` is invalid: {error}", rule.id)
                    }),
            })
            .collect();
        let (keyword_rules, keywords): (Vec<usize>, Vec<&str>) = RULES
            .iter()
            .enumerate()
            .flat_map(|(index, rule)| rule.keywords.iter().map(move |keyword| (index, *keyword)))
            .unzip();
        let keywords = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(keywords)
            .expect("secret rule keywords are valid");
        Self {
            rules,
            keywords,
            keyword_rules,
        }
    }

    /// Rules whose keywords appear in `text` and that `config` enables.
    fn candidates(&self, text: &str, config: &SecretRedactionConfig) -> Vec<&CompiledRule> {
        let mut selected = vec![false; self.rules.len()];
        for found in self.keywords.find_overlapping_iter(text) {
            selected[self.keyword_rules[found.pattern().as_usize()]] = true;
        }
        self.rules
            .iter()
            .zip(selected)
            .filter(|(compiled, selected)| *selected && config.enables(compiled.rule))
            .map(|(compiled, _)| compiled)
            .collect()
    }
}

struct Finding {
    span: Range<usize>,
    rule_id: &'static str,
}

/// Replaces every detected secret in `text` with `[REDACTED:<rule_id>]`.
///
/// Returns the redacted text and the IDs of the rules that matched, or `None`
/// when nothing was redacted.
pub(super) fn redact_text(
    text: &str,
    config: &SecretRedactionConfig,
) -> Option<(String, Vec<&'static str>)> {
    let mut findings = find_secrets(text, config);
    if findings.is_empty() {
        return None;
    }
    findings.sort_by_key(|finding| (finding.span.start, std::cmp::Reverse(finding.span.end)));

    let mut redacted = String::with_capacity(text.len());
    let mut rule_ids = Vec::new();
    let mut cursor = 0;
    for finding in findings {
        if !rule_ids.contains(&finding.rule_id) {
            rule_ids.push(finding.rule_id);
        }
        if finding.span.start < cursor {
            // Overlaps an earlier finding: widen the redaction instead of
            // leaving a partial secret behind.
            cursor = cursor.max(finding.span.end);
            continue;
        }
        redacted.push_str(&text[cursor..finding.span.start]);
        redacted.push_str("[REDACTED:");
        redacted.push_str(finding.rule_id);
        redacted.push(']');
        cursor = finding.span.end;
    }
    redacted.push_str(&text[cursor..]);
    Some((redacted, rule_ids))
}

fn find_secrets(text: &str, config: &SecretRedactionConfig) -> Vec<Finding> {
    let mut findings = Vec::new();
    for compiled in SCANNER.candidates(text, config) {
        for captures in compiled.regex.captures_iter(text) {
            let Some(secret) = captures.get(1).or_else(|| captures.get(0)) else {
                continue;
            };
            if is_secret(compiled.rule, secret.as_str()) {
                findings.push(Finding {
                    span: secret.range(),
                    rule_id: compiled.rule.id,
                });
            }
        }
    }
    findings
}

fn is_secret(rule: &Rule, value: &str) -> bool {
    if rule.reject_word_like
        && value
            .chars()
            .all(|char| char.is_ascii_alphabetic() || matches!(char, '_' | '.' | '-'))
    {
        return false;
    }
    shannon_entropy(value) >= rule.min_entropy && !is_placeholder(value)
}

/// Rejects documentation values and template references that match a rule's
/// shape but cannot be live credentials.
fn is_placeholder(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    const MARKERS: &[&str] = &["example", "your_", "your-", "<", ">", "${", "{{", "}}"];
    if MARKERS.iter().any(|marker| lowered.contains(marker)) {
        return true;
    }
    if value.len() > 2 && value.starts_with('%') && value.ends_with('%') {
        return true;
    }
    most_common_byte_count(value) * 2 > value.len()
}

fn most_common_byte_count(value: &str) -> usize {
    let mut counts = [0_usize; 256];
    for byte in value.bytes() {
        counts[usize::from(byte)] += 1;
    }
    counts.into_iter().max().unwrap_or(0)
}

fn shannon_entropy(value: &str) -> f32 {
    if value.is_empty() {
        return 0.0;
    }
    let mut counts = [0_u32; 256];
    for byte in value.bytes() {
        counts[usize::from(byte)] += 1;
    }
    let len = value.len() as f32;
    counts
        .into_iter()
        .filter(|count| *count > 0)
        .map(|count| {
            let probability = count as f32 / len;
            -probability * probability.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_compiles() {
        assert_eq!(SCANNER.rules.len(), RULES.len());
    }

    #[test]
    fn rule_ids_are_unique_and_valid_reason_suffixes() {
        let mut ids = std::collections::BTreeSet::new();
        for rule in RULES {
            assert!(ids.insert(rule.id), "duplicate rule `{}`", rule.id);
            assert!(
                rule.id
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
                "rule `{}` must be kebab-case",
                rule.id
            );
            assert!(
                !rule.keywords.is_empty(),
                "rule `{}` needs keywords",
                rule.id
            );
        }
    }

    #[test]
    fn entropy_distinguishes_repetition_from_random_text() {
        assert_eq!(shannon_entropy("aaaaaaaa"), 0.0);
        assert!(shannon_entropy("aB3dE6gH9jK2mN5pQ8sT") > 4.0);
    }

    #[test]
    fn placeholders_are_rejected() {
        for value in [
            "AKIAIOSFODNN7EXAMPLE",
            "sk-your_key_here",
            "<OPENAI_API_KEY>",
            "${OPENAI_API_KEY}",
            "{{ secrets.token }}",
            "%API_KEY%",
            "xxxxxxxxxxxxxxxxxxxx",
        ] {
            assert!(is_placeholder(value), "{value} should be a placeholder");
        }
        assert!(!is_placeholder("aB3dE6gH9jK2mN5pQ8sT"));
    }
}
