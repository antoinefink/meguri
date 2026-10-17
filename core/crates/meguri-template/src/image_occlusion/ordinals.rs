//! Pure editor numbering (render/image-occlusion-format F7.4, F12).
use super::Shape;
use std::collections::{BTreeMap, BTreeSet};

/// None means all encodable positive cloze numbers are occupied.
pub fn allocate(shapes: &[Shape]) -> Option<u16> {
    let used: BTreeSet<_> = shapes.iter().map(|s| s.number).collect();
    (1..=u16::MAX).find(|n| !used.contains(n))
}

pub fn groups(shapes: &[Shape]) -> BTreeMap<u16, Vec<usize>> {
    let mut groups: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (i, shape) in shapes.iter().enumerate() {
        if !shape.kind.is_text() && shape.number > 0 {
            groups.entry(shape.number).or_default().push(i);
        }
    }
    groups.retain(|_, members| members.len() >= 2);
    groups
}

/// Indices outside the document are ignored; repeated selections count once.
pub fn group(shapes: &mut [Shape], selected: &[usize]) {
    let selected: BTreeSet<_> = selected
        .iter()
        .copied()
        .filter(|&i| shapes.get(i).is_some_and(|s| !s.kind.is_text()))
        .collect();
    if selected.len() < 2 {
        return;
    }
    let number = selected.iter().map(|&i| shapes[i].number).min().expect("two masks");
    for i in selected {
        shapes[i].number = number;
    }
}

/// Allocation failure is atomic: no partial ungroup is exposed.
pub fn ungroup(shapes: &mut [Shape], number: u16) -> Option<()> {
    let mut result = shapes.to_vec();
    let members = groups(shapes).remove(&number).unwrap_or_default();
    for &index in members.iter().skip(1) {
        result[index].number = allocate(&result)?;
    }
    shapes.clone_from_slice(&result);
    Some(())
}

/// Copies in editor order. All copies of a positive-number group share one
/// fresh number; separate masks and labels receive their own F12 number.
pub fn duplicate(shapes: &[Shape], selected: &[usize]) -> Option<Vec<Shape>> {
    let selected: BTreeSet<_> = selected.iter().copied().collect();
    let mut occupied = shapes.to_vec();
    let mut copied_groups = BTreeMap::new();
    let mut copies = Vec::new();
    for (_, shape) in shapes.iter().enumerate().filter(|(i, _)| selected.contains(i)) {
        let mut copy = shape.clone();
        copy.source = None;
        copy.number = if shape.kind.is_text() {
            0
        } else if shape.number > 0 {
            match copied_groups.get(&shape.number) {
                Some(&number) => number,
                None => {
                    let number = allocate(&occupied)?;
                    copied_groups.insert(shape.number, number);
                    number
                }
            }
        } else {
            allocate(&occupied)?
        };
        occupied.push(copy.clone());
        copies.push(copy);
    }
    Some(copies)
}
