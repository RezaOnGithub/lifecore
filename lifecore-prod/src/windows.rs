//! Reviews, windows, and seals (PLANNING §5).
//!
//! Reviews are numbered from 1 in append order. Window 0 runs up to and
//! including Review 1; window k runs after Review k up to and including
//! Review k+1. So Review k seals window k-1 and opens window k, and an entry
//! appended in window k is sealed by Review k+1.

use crate::vocabulary::PlanningType;
use lifecore::{Node, View};

pub(crate) struct Reviews<'a> {
    // Every review of the view, in append order.
    reviews: Vec<&'a Node>,
}

impl<'a> Reviews<'a> {
    pub fn new(view: View<'a>) -> Self {
        Self {
            reviews: view
                .nodes()
                .filter(|node| node.ty_name == PlanningType::Review.name())
                .collect(),
        }
    }
    pub fn count(&self) -> usize {
        self.reviews.len()
    }
    // Review `number`, counted from 1.
    pub fn review(&self, number: usize) -> &'a Node {
        self.reviews[number - 1]
    }
    pub fn latest(&self) -> Option<&'a Node> {
        self.reviews.last().copied()
    }
    pub fn all(&self) -> impl Iterator<Item = &'a Node> + '_ {
        self.reviews.iter().copied()
    }
    // The window of the entry at `position`: the number of reviews before it.
    pub fn window_of(&self, position: usize) -> usize {
        self.reviews
            .iter()
            .take_while(|review| review.id.position() < position)
            .count()
    }
    // The number of the review that seals the entry at `position`, the first
    // review after it, if the view holds one. A review's own position reads
    // as sealed by that review, the last entry of the window it seals.
    pub fn sealing_review(&self, position: usize) -> Option<usize> {
        let number = self.window_of(position) + 1;
        (number <= self.count()).then_some(number)
    }
}

// The boundary at which the terms of §6 and §7 are judged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Boundary {
    // The prefix whose newest entry is Review `n`, which seals window n-1.
    Seal(usize),
    // A boundary in the open window `k`, after Review k; the valuation is
    // always judged here, even when the newest entry is a review.
    Open(usize),
}

impl Boundary {
    // The window whose entries are fresh: the one being sealed, or the open
    // one.
    pub fn window(self) -> usize {
        match self {
        Self::Seal(review) => review - 1,
        Self::Open(window) => window,
        }
    }
}
