use crate::*;
use AutomatonDecision::*;

/// Decisions of the Automaton on one axis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AutomatonDecision {
    UnbalancedPair {
        pos: bool,
        att_dist: usize,
        rep_dist: usize,
    },
    FromRepulsor {
        pos: bool,
        rep_dist: usize,
    },
    TowardAttractor {
        pos: bool,
        att_dist: usize,
    },
    None,
}

#[instrument]
fn evaluate_axis(pos: &Raycast, neg: &Raycast) -> AutomatonDecision {
    use Particle::{Attractor as A, Repulsor as R, Vacuum as V};

    // note that whenever we check `dist > 1`, we are ensuring there is
    // at least 1 empty space along that direction before the particle,
    // so that there is space for the automaton to move. the distance
    // checks are always in the direction the automaton would move, and
    // are symmetric.
    match (pos.what, neg.what) {
        (A, R) if pos.dist > 1 => UnbalancedPair {
            pos: true,
            att_dist: pos.dist,
            rep_dist: neg.dist,
        },
        (R, A) if neg.dist > 1 => UnbalancedPair {
            pos: false,
            att_dist: neg.dist,
            rep_dist: pos.dist,
        },
        (R, R) if pos.dist != neg.dist => FromRepulsor {
            pos: pos.dist > neg.dist,
            rep_dist: std::cmp::min(pos.dist, neg.dist),
        },
        (R, V) if neg.dist > 1 => FromRepulsor {
            pos: false,
            rep_dist: pos.dist,
        },
        (V, R) if pos.dist > 1 => FromRepulsor {
            pos: true,
            rep_dist: neg.dist,
        },
        (A, A) if pos.dist != neg.dist && std::cmp::min(pos.dist, neg.dist) > 1 => {
            TowardAttractor {
                pos: pos.dist < neg.dist,
                att_dist: std::cmp::min(pos.dist, neg.dist),
            }
        }
        (A, V) if pos.dist > 1 => TowardAttractor {
            pos: true,
            att_dist: pos.dist,
        },
        (V, A) if neg.dist > 1 => TowardAttractor {
            pos: false,
            att_dist: neg.dist,
        },
        _ => None,
    }
}

impl Game {
    /// Update the automaton, returning true if it moved
    pub fn update_automaton(&mut self) {
        let new_location = self.automaton_move();
        if new_location != self.board.automaton_location {
            debug_assert_eq!(
                self.board
                    .do_move(self.board.automaton_location, new_location),
                MoveResult::Applied
            );
        }
    }

    /// Calculate the coordinate to which the automaton would move right now.
    #[instrument]
    pub(crate) fn automaton_move(&self) -> Coord {
        /// Find the nearest particles in the four directions.
        let xp = self.board.raycast(self.board.automaton_location, Delta::XP);
        let xn = self.board.raycast(self.board.automaton_location, Delta::XN);
        let yp = self.board.raycast(self.board.automaton_location, Delta::YP);
        let yn = self.board.raycast(self.board.automaton_location, Delta::YN);

        let x_decision = evaluate_axis(&xp, &xn);
        let y_decision = evaluate_axis(&yp, &yn);

        let offset: Delta = if x_decision > y_decision {
            x_decision.delta(Delta::XP)
        } else if y_decision > x_decision {
            y_decision.delta(Delta::YP)
        } else {
            // Equal priority: apply column rule if enabled
            if self.use_column_rule {
                y_decision.delta(Delta::YP) // Column rule: prefer Y axis
            } else {
                info!("avoided applying the column rule - no move");
                Delta::ZERO
            }
        };

        self.board.automaton_location + offset
    }
}

fn tiebreaker(this: &AutomatonDecision, other: &AutomatonDecision) -> Ordering {
    fn chose_smaller_distance(a: &usize, b: &usize) -> Ordering {
        a.cmp(&b).reverse()
    }

    match (this, other) {
        // same priority means same enum variant!
        (
            UnbalancedPair {
                att_dist, rep_dist, ..
            },
            UnbalancedPair {
                att_dist: o_att_dist,
                rep_dist: o_rep_dist,
                ..
            },
        ) => chose_smaller_distance(att_dist, o_att_dist)
            .then(chose_smaller_distance(rep_dist, o_rep_dist)),
        (
            FromRepulsor { rep_dist, .. },
            FromRepulsor {
                rep_dist: o_rep_dist,
                ..
            },
        ) => chose_smaller_distance(rep_dist, o_rep_dist),
        (
            TowardAttractor { att_dist, .. },
            TowardAttractor {
                att_dist: o_att_dist,
                ..
            },
        ) => chose_smaller_distance(att_dist, o_att_dist),
        (None, None) => Ordering::Equal,
        _ => unreachable!(),
    }
}

impl Ord for AutomatonDecision {
    fn cmp(&self, other: &AutomatonDecision) -> Ordering {
        self.priority()
            .cmp(&other.priority())
            .then_with(|| tiebreaker(self, other))
    }
}

impl AutomatonDecision {
    pub(crate) fn priority(&self) -> usize {
        match self {
            UnbalancedPair { .. } => 30,
            // "Frank correction": this is higher priority
            FromRepulsor { .. } => 20,
            TowardAttractor { .. } => 10,
            None => 0,
        }
    }

    pub(crate) fn delta(&self, axis: Delta) -> Delta {
        fn sgn(&b: &bool) -> isize {
            if b { 1 } else { -1 }
        }

        match self {
            UnbalancedPair { pos, .. } | FromRepulsor { pos, .. } | TowardAttractor { pos, .. } => {
                axis.scale(sgn(pos))
            }
            None => Delta::ZERO,
        }
    }
}

impl PartialOrd for AutomatonDecision {
    fn partial_cmp(&self, other: &AutomatonDecision) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for AutomatonDecision {
    fn eq(&self, other: &AutomatonDecision) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for AutomatonDecision {}
