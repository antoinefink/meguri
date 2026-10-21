//! O5: each division and multiplication is binary32, with a strict comparison.
pub fn mix<T>(a: Vec<T>, b: Vec<T>, mode: i32) -> Vec<T> {
    let (an, bn) = (a.len(), b.len());
    let mut a = a.into_iter();
    let mut b = b.into_iter();
    let mut result = Vec::with_capacity(an + bn);
    if mode == 1 {
        result.extend(a);
        result.extend(b);
        return result;
    }
    if mode == 2 {
        result.extend(b);
        result.extend(a);
        return result;
    }
    let ratio = (an + 1) as f32 / (bn + 1) as f32;
    let (mut i, mut j) = (0usize, 0usize);
    while i < an && j < bn {
        if (j + 1) as f32 * ratio < (i + 1) as f32 {
            result.push(b.next().unwrap());
            j += 1;
        } else {
            result.push(a.next().unwrap());
            i += 1;
        }
    }
    result.extend(a);
    result.extend(b);
    result
}
