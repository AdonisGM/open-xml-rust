//! Grouping and ungrouping shapes (`p:grpSp`).

use openxml_core::{Error, Length, Result};
use openxml_schema::{dml, pml};

use crate::drawing;
use crate::shape;
use crate::slide::SlideMut;
use crate::util::{self, Bounds};

type C = pml::CT_GroupShape_Choice;

/// Rotation and flips of a shape-tree member.
fn orientation_mut(c: &mut C) -> Option<(&mut Option<i32>, &mut Option<bool>, &mut Option<bool>)> {
    fn of(x: &mut dml::CT_Transform2D) -> (&mut Option<i32>, &mut Option<bool>, &mut Option<bool>) {
        (&mut x.rot, &mut x.flip_h, &mut x.flip_v)
    }
    match c {
        C::Sp(s) => s.sp_pr.as_mut()?.xfrm.as_deref_mut().map(of),
        C::Pic(p) => p.sp_pr.as_mut()?.xfrm.as_deref_mut().map(of),
        C::CxnSp(x) => x.sp_pr.as_mut()?.xfrm.as_deref_mut().map(of),
        C::GraphicFrame(f) => f.xfrm.as_deref_mut().map(of),
        C::GrpSp(g) => g
            .grp_sp_pr
            .as_mut()?
            .xfrm
            .as_deref_mut()
            .map(|x| (&mut x.rot, &mut x.flip_h, &mut x.flip_v)),
        _ => None,
    }
}

/// Whether PowerPoint refuses to group a member: placeholders and shapes
/// locked with `noGrp` (such as tables).
fn cannot_group(c: &C) -> bool {
    match c {
        C::Sp(s) => {
            let nv = s.nv_sp_pr.as_deref();
            nv.and_then(|n| n.nv_pr.as_ref()).is_some_and(|p| p.ph.is_some())
                || nv
                    .and_then(|n| n.c_nv_sp_pr.as_ref())
                    .and_then(|c| c.sp_locks.as_ref())
                    .and_then(|l| l.no_grp)
                    == Some(true)
        }
        C::Pic(p) => {
            let nv = p.nv_pic_pr.as_deref();
            nv.and_then(|n| n.nv_pr.as_ref()).is_some_and(|p| p.ph.is_some())
                || nv
                    .and_then(|n| n.c_nv_pic_pr.as_ref())
                    .and_then(|c| c.pic_locks.as_ref())
                    .and_then(|l| l.no_grp)
                    == Some(true)
        }
        C::GraphicFrame(f) => {
            let nv = f.nv_graphic_frame_pr.as_deref();
            nv.and_then(|n| n.nv_pr.as_ref()).is_some_and(|p| p.ph.is_some())
                || nv
                    .and_then(|n| n.c_nv_graphic_frame_pr.as_ref())
                    .and_then(|c| c.graphic_frame_locks.as_ref())
                    .and_then(|l| l.no_grp)
                    == Some(true)
        }
        C::ContentPart(_) | C::Other(_) => true,
        C::GrpSp(_) | C::CxnSp(_) => false,
    }
}

fn union(bounds: &[Bounds]) -> Bounds {
    let x0 = bounds.iter().map(|b| b.0.as_emu()).min().unwrap_or(0);
    let y0 = bounds.iter().map(|b| b.1.as_emu()).min().unwrap_or(0);
    let x1 = bounds.iter().map(|b| (b.0 + b.2).as_emu()).max().unwrap_or(0);
    let y1 = bounds.iter().map(|b| (b.1 + b.3).as_emu()).max().unwrap_or(0);
    (
        Length::emu(x0),
        Length::emu(y0),
        Length::emu(x1 - x0),
        Length::emu(y1 - y0),
    )
}

/// Maps a rectangle from a group's child coordinate space to its parent's.
fn to_parent(b: Bounds, outer: Bounds, inner: Bounds) -> Bounds {
    let scale = |v: i64, num: i64, den: i64| -> i64 {
        if den == 0 {
            v
        } else {
            (i128::from(v) * i128::from(num) / i128::from(den)) as i64
        }
    };
    let (ox, oy, ow, oh) = (
        outer.0.as_emu(),
        outer.1.as_emu(),
        outer.2.as_emu(),
        outer.3.as_emu(),
    );
    let (ix, iy, iw, ih) = (
        inner.0.as_emu(),
        inner.1.as_emu(),
        inner.2.as_emu(),
        inner.3.as_emu(),
    );
    (
        Length::emu(ox + scale(b.0.as_emu() - ix, ow, iw)),
        Length::emu(oy + scale(b.1.as_emu() - iy, oh, ih)),
        Length::emu(scale(b.2.as_emu(), ow, iw)),
        Length::emu(scale(b.3.as_emu(), oh, ih)),
    )
}

/// Applies the group's rotation and flips to a child rectangle already in
/// parent coordinates: the child's centre is mirrored/rotated around the
/// group centre, and the child's own orientation is updated.
fn apply_orientation(
    child: &mut C,
    b: Bounds,
    group: Bounds,
    rot: i32,
    flip_h: bool,
    flip_v: bool,
) -> Bounds {
    let gcx = group.0.as_emu() as f64 + group.2.as_emu() as f64 / 2.0;
    let gcy = group.1.as_emu() as f64 + group.3.as_emu() as f64 / 2.0;
    let mut cx = b.0.as_emu() as f64 + b.2.as_emu() as f64 / 2.0;
    let mut cy = b.1.as_emu() as f64 + b.3.as_emu() as f64 / 2.0;
    if flip_h {
        cx = 2.0 * gcx - cx;
    }
    if flip_v {
        cy = 2.0 * gcy - cy;
    }
    if rot != 0 {
        let a = drawing::degrees(rot).to_radians();
        let (dx, dy) = (cx - gcx, cy - gcy);
        cx = gcx + dx * a.cos() - dy * a.sin();
        cy = gcy + dx * a.sin() + dy * a.cos();
    }
    if let Some((r, fh, fv)) = orientation_mut(child) {
        let mut own = r.unwrap_or(0);
        if flip_h != flip_v {
            // A single mirror reverses the direction of the child's rotation.
            own = -own;
        }
        let total = drawing::angle(drawing::degrees(own) + drawing::degrees(rot));
        *r = (total != 0).then_some(total);
        if flip_h {
            *fh = if fh.unwrap_or(false) { None } else { Some(true) };
        }
        if flip_v {
            *fv = if fv.unwrap_or(false) { None } else { Some(true) };
        }
    }
    let half_w = b.2.as_emu() / 2;
    let half_h = b.3.as_emu() / 2;
    (
        Length::emu(cx.round() as i64 - half_w),
        Length::emu(cy.round() as i64 - half_h),
        b.2,
        b.3,
    )
}

impl SlideMut<'_> {
    /// Groups top-level shapes. The group's child coordinate space equals the
    /// slide space (`chOff`/`chExt` = `off`/`ext` = the members' bounding box),
    /// so member coordinates do not change. Animations of the members are
    /// removed, as PowerPoint does. Placeholders and shapes locked against
    /// grouping (tables) are refused. Returns the group's shape identifier.
    ///
    /// ```
    /// use openxml_core::Length;
    /// use openxml_pptx::{LayoutKind, Presentation, ShapeKind, ShapeType};
    ///
    /// let mut deck = Presentation::new();
    /// let mut slide = deck.add_slide(LayoutKind::Blank)?;
    /// let a = slide.add_shape(ShapeType::Rect, Length::cm(1.0), Length::cm(1.0), Length::cm(2.0), Length::cm(2.0)).id();
    /// let b = slide.add_shape(ShapeType::Ellipse, Length::cm(4.0), Length::cm(1.0), Length::cm(2.0), Length::cm(2.0)).id();
    /// let group = slide.group(&[a, b])?;
    /// assert_eq!(slide.shapes()[0].kind, ShapeKind::Group);
    /// assert_eq!(slide.shapes()[0].size, Some((Length::cm(5.0), Length::cm(2.0))));
    /// slide.ungroup(group)?;
    /// assert_eq!(slide.shapes().len(), 2);
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn group(&mut self, shape_ids: &[u32]) -> Result<u32> {
        if shape_ids.is_empty() {
            return Err(Error::InvalidArgument("nothing to group".into()));
        }
        let id = self.next_id();
        let tree = self.tree_mut();
        let mut positions = Vec::with_capacity(shape_ids.len());
        for &sid in shape_ids {
            let pos = tree
                .choice
                .iter()
                .position(|c| util::choice_id(c) == Some(sid))
                .ok_or_else(|| Error::NotFound(format!("top-level shape {sid}")))?;
            if !positions.contains(&pos) {
                positions.push(pos);
            }
        }
        positions.sort_unstable();
        if let Some(&p) = positions.iter().find(|&&p| cannot_group(&tree.choice[p])) {
            return Err(Error::InvalidArgument(format!(
                "shape {} is a placeholder or locked against grouping",
                util::choice_id(&tree.choice[p]).unwrap_or(0)
            )));
        }
        let bounds: Vec<Bounds> = positions
            .iter()
            .map(|&p| {
                util::bounds(&tree.choice[p])
                    .ok_or_else(|| Error::InvalidArgument("a shape to group has no position".into()))
            })
            .collect::<Result<_>>()?;
        let bbox = union(&bounds);
        let insert_at = positions[0];
        let mut members = Vec::with_capacity(positions.len());
        for &p in positions.iter().rev() {
            members.push(tree.choice.remove(p));
        }
        members.reverse();
        let mut group = pml::CT_GroupShape {
            nv_grp_sp_pr: Some(Box::new(pml::CT_GroupShapeNonVisual {
                c_nv_pr: Some(Box::new(shape::nv_props(id, &format!("Group {}", id - 1)))),
                c_nv_grp_sp_pr: Some(Box::default()),
                nv_pr: Some(Box::default()),
                ..Default::default()
            })),
            choice: members,
            ..Default::default()
        };
        util::set_group_transform(&mut group, bbox, bbox);
        tree.choice.insert(insert_at, C::GrpSp(Box::new(group)));
        crate::animation::remove_targets(self.raw_mut(), shape_ids);
        Ok(id)
    }

    /// Dissolves a group (at any nesting depth), moving its members into the
    /// parent with coordinates mapped from the group's child space (including
    /// its rotation and flips). Animations of the group are removed. Returns
    /// the members' identifiers.
    pub fn ungroup(&mut self, group_id: u32) -> Result<Vec<u32>> {
        fn dissolve(tree: &mut pml::CT_GroupShape, id: u32) -> Option<Result<Vec<u32>>> {
            if let Some(pos) = tree
                .choice
                .iter()
                .position(|c| util::choice_id(c) == Some(id) && matches!(c, C::GrpSp(_)))
            {
                let C::GrpSp(group) = tree.choice.remove(pos) else {
                    unreachable!("matched a group")
                };
                let group = *group;
                let xfrm = group.grp_sp_pr.as_ref().and_then(|p| p.xfrm.as_deref());
                let rot = xfrm.and_then(|x| x.rot).unwrap_or(0);
                let flip_h = xfrm.and_then(|x| x.flip_h).unwrap_or(false);
                let flip_v = xfrm.and_then(|x| x.flip_v).unwrap_or(false);
                let transform = util::group_transform(&group);
                let mut ids = Vec::new();
                let mut members = group.choice;
                for m in &mut members {
                    if let Some(i) = util::choice_id(m) {
                        ids.push(i);
                    }
                    if let (Some((outer, inner)), Some(b)) = (transform, util::bounds(m)) {
                        let mapped = to_parent(b, outer, inner);
                        let placed = apply_orientation(m, mapped, outer, rot, flip_h, flip_v);
                        util::set_bounds(m, placed);
                    }
                }
                for (k, m) in members.into_iter().enumerate() {
                    tree.choice.insert(pos + k, m);
                }
                return Some(Ok(ids));
            }
            tree.choice.iter_mut().find_map(|c| match c {
                C::GrpSp(g) => dissolve(g, id),
                _ => None,
            })
        }
        let ids = dissolve(self.tree_mut(), group_id)
            .unwrap_or_else(|| Err(Error::NotFound(format!("group {group_id}"))))?;
        crate::animation::remove_targets(self.raw_mut(), &[group_id]);
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x: i64, y: i64, w: i64, h: i64) -> Bounds {
        (Length::emu(x), Length::emu(y), Length::emu(w), Length::emu(h))
    }

    #[test]
    fn bounding_boxes() {
        assert_eq!(union(&[b(0, 10, 5, 5), b(20, 0, 5, 30)]), b(0, 0, 25, 30));
    }

    #[test]
    fn child_space_mapping() {
        // Child space twice as large as the group: coordinates are halved.
        let outer = b(100, 100, 50, 50);
        let inner = b(0, 0, 100, 100);
        assert_eq!(to_parent(b(20, 40, 10, 10), outer, inner), b(110, 120, 5, 5));
        // Degenerate child extents keep coordinates unscaled.
        assert_eq!(
            to_parent(b(1, 1, 1, 1), b(0, 0, 0, 0), b(0, 0, 0, 0)),
            b(1, 1, 1, 1)
        );
    }

    #[test]
    fn orientation_is_applied_around_the_group_centre() {
        let mut sp = C::Sp(Box::new(shape::new_auto_shape(
            2,
            dml::ST_ShapeType::Rect,
            Length::ZERO,
            Length::ZERO,
            Length::emu(10),
            Length::emu(10),
        )));
        let group = b(0, 0, 100, 100);
        // Rotating by 90° moves the top-left child to the top-right.
        let placed = apply_orientation(
            &mut sp,
            b(0, 0, 10, 10),
            group,
            drawing::angle(90.0),
            false,
            false,
        );
        assert_eq!(placed, b(90, 0, 10, 10));
        let C::Sp(s) = &sp else { panic!() };
        assert_eq!(
            s.sp_pr.as_ref().unwrap().xfrm.as_ref().unwrap().rot,
            Some(5_400_000)
        );
        // A horizontal flip mirrors the centre and toggles the child's flip.
        let placed = apply_orientation(&mut sp, b(0, 0, 10, 10), group, 0, true, false);
        assert_eq!(placed, b(90, 0, 10, 10));
        let C::Sp(s) = &sp else { panic!() };
        let x = s.sp_pr.as_ref().unwrap().xfrm.as_ref().unwrap();
        assert_eq!(x.flip_h, Some(true));
        assert_eq!(x.rot, Some(16_200_000), "mirroring reverses the rotation");
    }
}
