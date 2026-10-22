//! O5: retain stored f32 lists until their text is edited.
use super::{Code, Field, Issue};

pub fn parse(text: &str, field: Field, preset: Option<i64>) -> Result<Vec<f32>, Vec<Issue>> {
    let mut steps = Vec::new();
    let mut issues = Vec::new();
    for (index, token) in text.split_whitespace().enumerate() {
        let (negative, unsigned) = token.strip_prefix('-').map_or((false, token), |s| (true, s));
        let (digits, multiplier) = match unsigned.as_bytes().last() {
            Some(b's') => (&unsigned[..unsigned.len() - 1], 1u64),
            Some(b'm') => (&unsigned[..unsigned.len() - 1], 60),
            Some(b'h') => (&unsigned[..unsigned.len() - 1], 3600),
            Some(b'd') => (&unsigned[..unsigned.len() - 1], 86400),
            _ => (unsigned, 60),
        };
        let seconds = digits
            .bytes()
            .try_fold(0u64, |n, b| {
                n.checked_mul(10)?.checked_add(u64::from(b.wrapping_sub(b'0')))
            })
            .and_then(|n| n.checked_mul(multiplier));
        let code = if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
            Some(Code::StepMalformed)
        } else if negative {
            Some(Code::StepNegative)
        } else {
            match seconds {
                Some(0) => Some(Code::StepZero),
                Some(n) if n <= 2_147_483_648 => None,
                _ => Some(Code::StepTooLong),
            }
        };
        if let Some(code) = code {
            let mut issue = Issue::error(field, preset, code);
            issue.params.insert("index".into(), index.to_string());
            issue.params.insert("token".into(), token.into());
            issues.push(issue);
        } else {
            steps.push((seconds.expect("valid length") as f64 / 60.0) as f32);
        }
    }
    if issues.is_empty() { Ok(steps) } else { Err(issues) }
}

pub fn format(steps: &[f32]) -> String {
    steps
        .iter()
        .map(|&minutes| {
            if !minutes.is_finite() || minutes <= 0.0 {
                return "invalid".into();
            }
            let seconds = f64::from(minutes) * 60.0;
            for (unit, label) in [(86400.0, "d"), (3600.0, "h"), (60.0, "m")] {
                if unit <= seconds && (seconds - (seconds / unit).round() * unit).abs() < f64::EPSILON {
                    return format!("{}{label}", (seconds / unit).round());
                }
            }
            format!("{}s", seconds.round())
        })
        .collect::<Vec<_>>()
        .join(" ")
}
