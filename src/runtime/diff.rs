//! Which parts of the screen changed between two frames' draw commands.
//!
//! The new list is matched against the old one as a shortest edit script
//! (Myers' diff, as in `git diff`): commands kept in order are unchanged,
//! everything inserted or deleted is damaged. A pixel outside all damaged
//! commands is covered by the same commands in the same order in both frames,
//! so it cannot have changed. Inserting one command no longer shifts every
//! command after it into the damage, as an index-by-index comparison did.

use crate::alloc_prelude::*;
use crate::graphics::{Damage, DrawCommand};

/// Edit distance beyond which the changed range is damaged as a whole; the
/// search costs O((old + new) * d) time and O(d²) memory.
const MAX_EDITS: usize = 64;

/// Adds the bounds of every command that differs between `old` and `new`.
pub(crate) fn damage_between(old: &[DrawCommand], new: &[DrawCommand], damage: &mut Damage) {
    // Most frames change a few commands somewhere in the middle.
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let (old, new) = (&old[prefix..], &new[prefix..]);
    let suffix = old.iter().rev().zip(new.iter().rev()).take_while(|(a, b)| a == b).count();
    let (old, new) = (&old[..old.len() - suffix], &new[..new.len() - suffix]);

    let mut add = |cmd: &DrawCommand| {
        if let Some(bounds) = cmd.bounds() {
            damage.add(bounds);
        }
    };
    match matches(old, new) {
        Some((kept_old, kept_new)) => {
            old.iter().zip(&kept_old).chain(new.iter().zip(&kept_new)).filter(|(_, kept)| !**kept).for_each(|(cmd, _)| add(cmd));
        }
        None => old.iter().chain(new).for_each(add),
    }
}

/// Myers' shortest edit script: which elements of `a` and `b` are part of the
/// longest common subsequence it keeps. `None` if more than `MAX_EDITS` edits
/// are needed.
fn matches(a: &[DrawCommand], b: &[DrawCommand]) -> Option<(Vec<bool>, Vec<bool>)> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = ((n + m) as usize).min(MAX_EDITS) as isize;
    // v[k] = furthest x on diagonal k = x - y, stored at k + max.
    let at = |k: isize| (k + max) as usize;
    let mut v = vec![0isize; 2 * max as usize + 2];
    // The v of every step before d, restricted to diagonals -d..=d.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    'search: for d in 0..=max {
        trace.push(v[at(-d)..=at(d)].to_vec());
        for k in (-d..=d).step_by(2) {
            let mut x = if k == -d || (k != d && v[at(k - 1)] < v[at(k + 1)]) { v[at(k + 1)] } else { v[at(k - 1)] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[at(k)] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'search;
            }
        }
    }
    let found = found?;

    let mut kept_a = vec![false; a.len()];
    let mut kept_b = vec![false; b.len()];
    let (mut x, mut y) = (n, m);
    for d in (0..=found).rev() {
        let row = &trace[d as usize];
        let get = |k: isize| row[(k + d) as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && get(k - 1) < get(k + 1)) { k + 1 } else { k - 1 };
        let (prev_x, prev_y) = if d == 0 { (0, 0) } else { (get(prev_k), get(prev_k) - prev_k) };
        while x > prev_x && y > prev_y {
            kept_a[(x - 1) as usize] = true;
            kept_b[(y - 1) as usize] = true;
            x -= 1;
            y -= 1;
        }
        x = prev_x;
        y = prev_y;
    }
    Some((kept_a, kept_b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{Color, Rect};

    fn rect(x: f32, c: u8) -> DrawCommand {
        DrawCommand::DrawRect {
            rect: Rect::new(x, 0.0, 10.0, 10.0),
            color: Some(Color::new(c, 0, 0, 255)),
            gradient: None,
            border_radius: 0.0,
            border: crate::style::Borders::NONE,
        }
    }

    fn kept(a: &[DrawCommand], b: &[DrawCommand]) -> (Vec<bool>, Vec<bool>) {
        matches(a, b).expect("within MAX_EDITS")
    }

    #[test]
    fn an_insertion_keeps_everything_after_it() {
        let old: Vec<_> = (0..6).map(|i| rect(i as f32 * 100.0, i)).collect();
        let mut new = old.clone();
        new.insert(2, rect(1000.0, 99));
        let (ka, kb) = kept(&old, &new);
        assert!(ka.iter().all(|k| *k));
        assert_eq!(kb, [true, true, false, true, true, true, true]);
    }

    #[test]
    fn changes_and_deletions_are_not_kept() {
        let old: Vec<_> = (0..5).map(|i| rect(i as f32 * 100.0, i)).collect();
        let new = vec![old[0].clone(), rect(100.0, 50), old[3].clone(), old[4].clone()];
        let (ka, kb) = kept(&old, &new);
        assert_eq!(ka, [true, false, false, true, true]);
        assert_eq!(kb, [true, false, true, true]);
    }

    #[test]
    fn a_swap_damages_the_moved_command() {
        let old = vec![rect(0.0, 1), rect(0.0, 2)];
        let new = vec![rect(0.0, 2), rect(0.0, 1)];
        let (ka, kb) = kept(&old, &new);
        assert_eq!(ka.iter().filter(|k| **k).count(), 1);
        assert_eq!(kb.iter().filter(|k| **k).count(), 1);
    }

    #[test]
    fn damage_covers_only_the_inserted_command() {
        let old: Vec<_> = (0..6).map(|i| rect(i as f32 * 100.0, i)).collect();
        let mut new = old.clone();
        new.insert(3, rect(1000.0, 99));
        let mut damage = Damage::none();
        damage_between(&old, &new, &mut damage);
        assert_eq!(damage.rects(), [new[3].bounds().unwrap().round_out()]);
    }

    #[test]
    fn too_many_edits_damage_the_whole_changed_range() {
        let old: Vec<_> = (0..100).map(|i| rect(i as f32, i as u8)).collect();
        let new: Vec<_> = (0..100).map(|i| rect(i as f32, 200 - i as u8)).collect();
        assert!(matches(&old, &new).is_none());
        let mut damage = Damage::none();
        damage_between(&old, &new, &mut damage);
        assert_eq!(damage.bounds(), old.iter().filter_map(DrawCommand::bounds).reduce(|a, b| a.expand(b)).map(|r| r.round_out()));
    }
}
