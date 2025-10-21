use automatafl_logic::*;

fn main() {
    let mut game = Game::new(
        Board::stock_two_player(),
        2,
        true,
        MergeResolutionMode::BunchedStacking,
        CycleBehaviorMode::RotatePieces,
    );
    let mut line = String::new();
    let stdin = std::io::stdin();
    while !game.winner.is_some() {
        println!("game state: {:?}", game.board);

        line.clear();

        stdin.read_line(&mut line).expect("i/o why u fail :(");

        let mut spl = line.split(' ');

        let pid = spl.next().expect("need pid").parse::<u8>().unwrap();
        let srcx = spl.next().expect("need srcx").parse::<u8>().unwrap();
        let srcy = spl.next().expect("need srcy").parse::<u8>().unwrap();
        let dstx = spl.next().expect("need dstx").parse::<u8>().unwrap();
        let dsty = spl.next().expect("need dsty").parse::<u8>().unwrap();

        let fdb = game.propose_move(Move {
            who: Pid(pid),
            from: Coord { x: srcx, y: srcy },
            to: Coord { x: dstx, y: dsty },
        });
        println!("Move feedback: {}", fdb);
        if matches!(fdb, ProposeFeedback::AcceptedAndReady) {
            match game.try_complete_round() {
                CompleteRoundFeedback::CompletedMoves(completed_moves) => {
                    for (m, res) in completed_moves {
                        println!("Player {}: {}", m.who.0, res)
                    }
                }
                CompleteRoundFeedback::Conflict(conlict_info) => {
                    print!(
                        "Players locked: {}",
                        conlict_info
                            .locked_players
                            .iter()
                            .map(|p| p.0.to_string())
                            .collect::<Vec<String>>()
                            .join(", ")
                    );
                }
                CompleteRoundFeedback::WaitingForPlayers(_) => {
                    unreachable!("ProposeFeedback::AcceptedAndReady implies all moves are in")
                }
            }
        }
    }

    println!("Player {} wins!", game.winner.unwrap().0);
}
