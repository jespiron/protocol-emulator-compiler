//! Finite-pool assignment. A successful result is an actual assignment witness.
use crate::CompileError as Error;
use std::collections::{BTreeMap, BTreeSet};

/// Augmenting paths, with deterministic name/pad order matching the reference.
pub(crate) fn pins(
    candidates: &BTreeMap<String, Vec<usize>>,
    capacity: usize,
) -> Result<BTreeMap<String, usize>, Error> {
    if candidates
        .values()
        .any(|p| p.is_empty() || p.iter().any(|&n| n >= capacity))
    {
        return Err(Error::new(
            "pin-pad",
            format!(
                "each pin needs candidates within 0..{}",
                capacity.saturating_sub(1)
            ),
        ));
    }
    fn augment<'a>(
        name: &'a str,
        candidates: &'a BTreeMap<String, Vec<usize>>,
        owner: &mut [Option<&'a str>],
        seen: &mut BTreeSet<usize>,
    ) -> bool {
        for pad in candidates[name].iter().copied().collect::<BTreeSet<_>>() {
            if !seen.insert(pad) {
                continue;
            }
            if owner[pad].is_none_or(|previous| augment(previous, candidates, owner, seen)) {
                owner[pad] = Some(name);
                return true;
            }
        }
        false
    }
    let mut owner = vec![None; capacity];
    for name in candidates.keys() {
        if !augment(name, candidates, &mut owner, &mut BTreeSet::new()) {
            return Err(Error::new(
                "pin-capacity",
                format!("cannot assign distinct pads to {candidates:?}; available={capacity}"),
            ));
        }
    }
    Ok(owner
        .into_iter()
        .enumerate()
        .filter_map(|(pad, name)| name.map(|n| (n.to_owned(), pad)))
        .collect())
}

pub(crate) fn units<'a>(
    names: impl Iterator<Item = &'a str>,
    capacity: usize,
    kind: &str,
) -> Result<BTreeMap<String, usize>, Error> {
    let names: Vec<_> = names.collect();
    if names.len() > capacity {
        return Err(Error::new(
            "tile-capacity",
            format!(
                "{kind}: requested {}, available {capacity}: {names:?}",
                names.len()
            ),
        ));
    }
    Ok(names
        .into_iter()
        .enumerate()
        .map(|(k, n)| (n.to_owned(), k))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhaustive_matching() {
        let subsets: Vec<Vec<usize>> = (1..8)
            .map(|bits| (0..3).filter(|p| bits & (1 << p) != 0).collect())
            .collect();
        for a in &subsets {
            for b in &subsets {
                for c in &subsets {
                    let possible = a.iter().any(|x| {
                        b.iter()
                            .any(|y| c.iter().any(|z| x != y && y != z && x != z))
                    });
                    let candidates = BTreeMap::from([
                        ("a".into(), a.clone()),
                        ("b".into(), b.clone()),
                        ("c".into(), c.clone()),
                    ]);
                    let result = pins(&candidates, 3);
                    assert_eq!(result.is_ok(), possible, "{candidates:?}");
                    if let Ok(result) = result {
                        assert_eq!(result.values().collect::<BTreeSet<_>>().len(), 3);
                        assert!(result.iter().all(|(n, p)| candidates[n].contains(p)));
                    }
                }
            }
        }
    }
}
