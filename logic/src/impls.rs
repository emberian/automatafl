use crate::*;

impl core::fmt::Display for CoordsFeedback {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        for (coord, feedback) in &self.data {
            write!(f, "{} {}", coord, feedback)?
        }
        Ok(())
    }
}

impl core::fmt::Display for Coord {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

impl std::ops::Sub for Coord {
    type Output = Delta;

    fn sub(self, other: Coord) -> Delta {
        Delta {
            dx: (self.x as i16 - other.x as i16),
            dy: (self.y as i16 - other.y as i16),
        }
    }
}

impl std::ops::Add<Delta> for Coord {
    type Output = Coord;

    fn add(self, other: Delta) -> Coord {
        Coord {
            // TODO: Make this saturate rather than wrap
            x: (self.x as i16 + other.dx) as u8,
            y: (self.y as i16 + other.dy) as u8,
        }
    }
}

impl Coord {
    pub fn ix(self) -> (usize, usize) {
        (self.y as usize, self.x as usize)
    }
}

impl Delta {
    pub(crate) const ZERO: Delta = Delta { dx: 0, dy: 0 };
    pub(crate) const XP: Delta = Delta { dx: 1, dy: 0 };
    pub(crate) const XN: Delta = Delta { dx: -1, dy: 0 };
    pub(crate) const YP: Delta = Delta { dx: 0, dy: 1 };
    pub(crate) const YN: Delta = Delta { dx: 0, dy: -1 };
    #[cfg(test)]
    pub(crate) const AXIAL_UNITS: [Delta; 4] = [Delta::XP, Delta::XN, Delta::YP, Delta::YN];

    pub(crate) fn is_zero(self) -> bool {
        self.dx == 0 && self.dy == 0
    }

    pub(crate) fn is_axial(self) -> bool {
        self.dx == 0 || self.dy == 0 && !self.is_zero()
    }

    pub(crate) fn axial_unit(self) -> Delta {
        if self.is_zero() {
            Delta::ZERO
        } else {
            // Fencepost: prefer Y ("column rule"). This shouldn't be relied upon; in general, call
            // this only on axial deltas.
            if !self.is_axial() {
                error!("{:?} is not an axial unit", self);
            }
            if self.dx.abs() > self.dy.abs() {
                Delta {
                    dx: self.dx.signum(),
                    dy: 0,
                }
            } else {
                Delta {
                    dx: 0,
                    dy: self.dy.signum(),
                }
            }
        }
    }

    pub(crate) fn displacement(self) -> usize {
        self.dx.abs() as usize + self.dy.abs() as usize
    }

    pub fn scale(self, factor: isize) -> Delta {
        Delta {
            dx: self.dx * (factor as i16),
            dy: self.dy * (factor as i16),
        }
    }

    #[cfg(test)]
    pub(crate) fn perpendicular(self) -> Delta {
        Delta {
            dx: -self.dy,
            dy: self.dx,
        }
    }
}

impl Particle {
    pub(crate) fn is_vacuum(self) -> bool {
        self == Particle::Vacuum
    }
}

impl Cell {
    pub(crate) fn occludes(&self) -> bool {
        // Vacuum can always be passed through, non-vacuum if passable is set.
        !(self.what.is_vacuum() || self.passable)
    }
}

impl AutomatonDecision {
    pub(crate) fn priority(&self) -> usize {
        use AutomatonDecision::*;
        match self {
            None => 0,
            TowardAttractor { .. } => 10,
            // "Frank correction": this is higher priority
            FromRepulsor { .. } => 20,
            UnbalancedPair { .. } => 30,
        }
    }

    pub(crate) fn delta(&self, axis: Delta) -> Delta {
        use AutomatonDecision::*;
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

impl core::fmt::Debug for Board {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        // --- Header ---
        writeln!(
            f,
            "Board ({}x{}) with automaton at {}",
            self.size.x, self.size.y, self.automaton_location
        )?;

        write!(f, "  ┌")?;
        for _ in 0..self.size.x {
            write!(f, "───")?;
        }
        writeln!(f, "┐")?;

        // --- Board Rows (from top to bottom) ---
        // Y-axis is printed from highest to lowest to match typical top-left origin consoles
        for y in (0..self.size.y).rev() {
            write!(f, "{:2}│", y)?;

            for x in 0..self.size.x {
                let cell = self.particles[Coord { x, y }.ix()];

                let conflict_char = if cell.conflict { '!' } else { ' ' };
                let passable_char = if cell.passable { '~' } else { ' ' };

                let particle_char = match cell.what {
                    Particle::Repulsor => 'R',
                    Particle::Attractor => 'A',
                    Particle::Automaton => 'D', // 'D' for Daemon/Automaton
                    Particle::Vacuum => '.',
                };

                // Format is: [Conflict Char][Particle Char][Passable Char]
                // e.g., " R ", "!A ", " .~"
                write!(f, "{}{}{}", conflict_char, particle_char, passable_char)?;
            }
            writeln!(f, "│")?;
        }

        write!(f, "  └")?;
        for _ in 0..self.size.x {
            write!(f, "───")?;
        }
        writeln!(f, "┘")?;
        write!(f, "    ")?; // Padding for row headers

        for x in 0..self.size.x {
            write!(f, "{:<3}", x)?;
        }
        writeln!(f)?;

        Ok(())
    }
}

impl Ord for AutomatonDecision {
    /// Which of these two automaton decisions is more urgent?
    fn cmp(&self, other: &AutomatonDecision) -> Ordering {
        use AutomatonDecision::*;

        // Yo, what's up with all these .reverse() calls?
        //
        // Well, the README.md describes the rules in a particular way, and to
        // make the code easy to verify, the code is written that way too.

        self.priority()
            .cmp(&other.priority())
            .then_with(|| match (self, other) {
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
                ) => att_dist
                    .cmp(o_att_dist)
                    .reverse()
                    .then(rep_dist.cmp(o_rep_dist).reverse()),
                (
                    FromRepulsor { rep_dist, .. },
                    FromRepulsor {
                        rep_dist: o_rep_dist,
                        ..
                    },
                ) => rep_dist.cmp(o_rep_dist).reverse(),
                (
                    TowardAttractor { att_dist, .. },
                    TowardAttractor {
                        att_dist: o_att_dist,
                        ..
                    },
                ) => att_dist.cmp(o_att_dist).reverse(),
                (None, None) => Ordering::Equal,
                _ => unreachable!(),
            })
    }
}

impl From<Particle> for Option<String> {
    fn from(particle: Particle) -> Self {
        match particle {
            Particle::Repulsor => Some("repulsor".to_string()),
            Particle::Attractor => Some("attractor".to_string()),
            Particle::Automaton => Some("automaton".to_string()),
            Particle::Vacuum => None,
        }
    }
}
