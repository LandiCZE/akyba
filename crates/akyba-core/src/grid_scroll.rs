//! The grid wider than the window: how wide it gets when every pane keeps a readable width, where
//! each pane sits across it, where a sideways scroll comes to rest, and which panes are out of
//! sight on either side. Distances are in pixels; `scroll` is how far the grid has moved left.

/// A pane's place across the grid, from its left edge to its right.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub left: f32,
    pub right: f32,
}

impl Span {
    pub fn width(self) -> f32 {
        self.right - self.left
    }

    fn center(self) -> f32 {
        f32::midpoint(self.left, self.right)
    }
}

/// Which side of the window a pane is hidden past.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// How wide the grid is: the window's width, or wider when the busiest row's panes would get
/// narrower than `narrowest`. A `narrowest` of zero squeezes every pane into the window, as Akyba
/// did before the grid scrolled.
#[expect(clippy::cast_precision_loss, reason = "a handful of panes")]
pub fn content_width(viewport: f32, panes_across: usize, narrowest: f32, gap: f32) -> f32 {
    if narrowest <= 0. || panes_across == 0 {
        return viewport;
    }
    let needed = narrowest * panes_across as f32 + gap * (panes_across - 1) as f32;
    viewport.max(needed)
}

/// Where a row's panes sit, laid out as the grid lays them out: the gaps between them are fixed,
/// and what's left is shared by their relative sizes.
#[expect(clippy::cast_precision_loss, reason = "a handful of panes")]
pub fn spans(sizes: &[f32], width: f32, gap: f32) -> Vec<Span> {
    let total: f32 = sizes.iter().sum();
    if sizes.is_empty() || total <= 0. {
        return Vec::new();
    }
    let shared = (width - gap * (sizes.len() - 1) as f32).max(0.);
    let mut left = 0.;
    sizes
        .iter()
        .map(|size| {
            let span = Span { left, right: left + shared * size / total };
            left = span.right + gap;
            span
        })
        .collect()
}

/// The side a pane is hidden on, or none while at least half of it shows: a pane half in sight
/// lights its own ring, so it needs no sign at the edge.
pub fn hidden_side(span: Span, scroll: f32, viewport: f32) -> Option<Side> {
    let shown = (span.right.min(scroll + viewport) - span.left.max(scroll)).max(0.);
    if shown * 2. >= span.width() {
        return None;
    }
    Some(if span.center() < scroll + viewport / 2. { Side::Left } else { Side::Right })
}

/// Where a scroll comes to rest: the nearest pane's left edge, or the very end, so the pane at
/// the window's left edge is never cut in half.
pub fn settle(scroll: f32, edges: &[f32], max: f32) -> f32 {
    edges
        .iter()
        .map(|edge| edge.clamp(0., max))
        .chain([0., max])
        .min_by(|a, b| (a - scroll).abs().total_cmp(&(b - scroll).abs()))
        .unwrap_or(scroll)
}

/// How far to scroll for a pane to be wholly in sight, moving as little as it can; none when it
/// already is. A pane wider than the window shows from its left edge.
pub fn reveal(span: Span, scroll: f32, viewport: f32, max: f32) -> Option<f32> {
    let in_sight = span.left >= scroll - 0.5 && span.right <= scroll + viewport + 0.5;
    if in_sight {
        return None;
    }
    let to = if span.left < scroll || span.width() > viewport { span.left } else { span.right - viewport };
    Some(to.clamp(0., max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panes_that_fit_keep_the_grid_as_wide_as_the_window() {
        assert!((content_width(1600., 3, 500., 4.) - 1600.).abs() < 0.01);
        assert!((content_width(1600., 6, 0., 4.) - 1600.).abs() < 0.01);
    }

    #[test]
    fn panes_that_would_be_too_narrow_widen_the_grid() {
        assert!((content_width(1600., 6, 500., 4.) - 3020.).abs() < 0.01);
    }

    #[test]
    fn spans_share_the_width_by_size_around_fixed_gaps() {
        let spans = spans(&[1., 1., 2.], 412., 4.);
        assert_eq!(spans.len(), 3);
        assert!((spans[0].left).abs() < 0.01 && (spans[0].right - 101.).abs() < 0.01);
        assert!((spans[1].left - 105.).abs() < 0.01);
        assert!((spans[2].right - 412.).abs() < 0.01);
    }

    #[test]
    fn a_pane_counts_as_hidden_once_less_than_half_shows() {
        let pane = Span { left: 1000., right: 1500. };
        assert_eq!(hidden_side(pane, 0., 1200.), Some(Side::Right));
        assert_eq!(hidden_side(pane, 100., 1200.), None);
        let first = Span { left: 0., right: 500. };
        assert_eq!(hidden_side(first, 300., 1200.), Some(Side::Left));
        assert_eq!(hidden_side(first, 200., 1200.), None);
    }

    #[test]
    fn a_scroll_rests_on_the_nearest_pane_edge_or_the_end() {
        let edges = [0., 504., 1008., 1512.];
        assert!((settle(230., &edges, 1400.)).abs() < 0.01);
        assert!((settle(300., &edges, 1400.) - 504.).abs() < 0.01);
        // The last edge is past the end, so the end is where it rests.
        assert!((settle(1450., &edges, 1400.) - 1400.).abs() < 0.01);
    }

    #[test]
    fn revealing_a_pane_moves_as_little_as_it_can() {
        let pane = Span { left: 1000., right: 1500. };
        assert_eq!(reveal(pane, 400., 1200., 2000.), None);
        assert_eq!(reveal(pane, 0., 1200., 2000.), Some(300.));
        assert_eq!(reveal(pane, 1200., 1200., 2000.), Some(1000.));
    }
}
