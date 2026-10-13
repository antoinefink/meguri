//! JSON pointers, pointer patterns and id patterns (observations.md O3,
//! dsl.md D9).

use serde_json::Value;

/// Renders unescaped segments as an RFC 6901 pointer.
pub fn render(segments: &[String]) -> String {
    segments
        .iter()
        .map(|s| format!("/{}", s.replace('~', "~0").replace('/', "~1")))
        .collect()
}

/// The unescaped segments of a pointer or pointer pattern.
pub fn parse(pointer: &str) -> Vec<String> {
    pointer
        .split('/')
        .skip(1)
        .map(|s| s.replace("~1", "/").replace("~0", "~"))
        .collect()
}

/// Calls `f` on every value whose pointer matches the pattern, where a `*`
/// segment matches any one member or item.
pub fn for_each_match(value: &mut Value, pattern: &[String], f: &mut dyn FnMut(&mut Value)) {
    let Some((head, rest)) = pattern.split_first() else {
        f(value);
        return;
    };
    match value {
        Value::Object(map) => {
            if head == "*" {
                map.values_mut().for_each(|v| for_each_match(v, rest, f));
            } else if let Some(v) = map.get_mut(head) {
                for_each_match(v, rest, f);
            }
        }
        Value::Array(items) => {
            if head == "*" {
                items.iter_mut().for_each(|v| for_each_match(v, rest, f));
            } else if let Some(v) = head.parse::<usize>().ok().and_then(|i| items.get_mut(i)) {
                for_each_match(v, rest, f);
            }
        }
        _ => {}
    }
}

/// An id pattern: `*` matches any run of characters (also `/`), `?` one
/// character, everything else itself.
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}
