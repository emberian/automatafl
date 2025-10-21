#![allow(unused_doc_comments)]

//! Reference implementation of the automatafl board game.
//!
//! General crate design notes:
//!
//! - Max board size is "small" (256x256) but easy to bump (Coord).
//! - Max player count is "small" (256) but easy to bump (Pid).
//! - `SmallVec` is used to size everything to require zero allocations
//!   during a standard four-goal, two-player game.
//! - Every error condition is uniquely identified and with
//!   nice Display implementations.
//! - Lots of state is public. If you EVER MUTATE ANYTHING, the game rules
//!   might break or the code might panic! Only calling methods will avoid this.
//!   Inspect state away :)

mod board;
mod game;
mod impls;
#[cfg(test)]
mod tests;
mod types;

pub use board::*;
pub use game::*;
pub use types::*;

use tracing::{error, info, instrument, trace};

use ndarray::arr2;
use smallvec::SmallVec;
use std::cmp::Ordering;
