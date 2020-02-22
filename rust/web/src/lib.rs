#![feature(track_caller)]
#![recursion_limit = "1024"]

use automatafl::*;

use moxie_dom::{
    elements::{button, div, img},
    prelude::*,
};
use wasm_bindgen::prelude::*;

#[topo::nested]
#[illicit::from_env(game: &Key<Game>)]
fn cell(c: Coord) {
    let game = game.clone(); // we'll need this if we click.

    fn next_particle(p: Particle) -> Particle {
        use Particle::*;
        match p {
            Repulsor => Attractor,
            Attractor => Automaton,
            Automaton => Vacuum,
            Vacuum => Repulsor,
        }
    }
    let cell: Cell = game.board.particles[c.ix()];
    let on_click = move |_: event::Click| {
        let mut g = Game::clone(&*game); // holy shit
        g.board.particles[c.ix()].what = next_particle(cell.what);
        game.set(g); // how can we avoid this
    };
    mox! {<div on={on_click }
    class={format!("cell passable-{} conflict-{}", cell.passable, cell.conflict)}><div style="display:inline-block"><img src={format!("img/{:?}.png", cell.what)}/></div></div>}
}

#[topo::nested]
#[illicit::from_env(game: &Key<Game>)]
fn game_board() {
    let button_game = game.clone();

    mox! {
        <div class="board">

        <button on={move |_: event::Click| {
            let mut g = Game::clone(&button_game);
            g.update_automaton();
            button_game.set(g);
        }}>"button time!"</button>

        {
            let Coord {x, y} = game.board.size;
            for rx in 0..x {
                mox!{
                    <div class="row">
                    {

                for ry in 0..y {
                    mox!{ <cell _={Coord{x:rx,y:ry}}/> }
                }

                    }
                    </div>
                }
            }
        } </div>
    }
}

#[topo::nested]
fn automatafl_game() {
    let game = state(|| Game::new(Board::stock_two_player(), 2, true));

    illicit::child_env![
        Key<Game> => game
    ]
    .enter(|| {
        topo::call(|| {
            mox! {
                <div class="game">
                    <game_board/>
                    //<move_states/>
                </div>
            }
        });
    });
}

#[wasm_bindgen(start)]
pub fn main() {
    console_log::init_with_level(tracing::log::Level::Debug).unwrap();
    std::panic::set_hook(Box::new(|info| {
        tracing::error!("{:#?}", info);
    }));
    tracing::info!("booting up!");
    moxie_dom::boot(document().body().unwrap(), automatafl_game);
}
