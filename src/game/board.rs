use super::{BoardSquare, Direction, GameWindow, Move, PieceColor, PieceData};
use slint::ComponentHandle;
use slint::{Model, Weak};
use std::hint;
use std::rc::Rc;

/// Struct holding gamestate of the checkers board
#[derive(Default, Clone)]
pub struct Board {
    game: Weak<GameWindow>,
    pieces: Rc<slint::VecModel<PieceData>>,
    player_color: PieceColor,
    squares: Rc<slint::VecModel<BoardSquare>>,
    pub selected_square: i32,
}

impl Board {
    pub fn new(game: &GameWindow) -> Board {
        let pieces = Rc::new(slint::VecModel::from(vec![]));

        let squares: Vec<BoardSquare> = vec![BoardSquare { marked: false }; 32];
        let squares = Rc::new(slint::VecModel::from(squares));
        game.set_squares(squares.clone().into());

        Board {
            game: game.as_weak(),
            pieces,
            squares,
            ..Default::default()
        }
    }

    /// Returns the starting setup of a checkers board based off `player_color`
    fn default_setup(player_color: PieceColor) -> Vec<PieceData> {
        let enemy_color = player_color.get_opposite();

        let mut tiles = vec![];

        for i in 0..32 {
            if i == 6 || i == 14 || i == 17 {
                tiles.push(PieceData {
                    color: enemy_color,
                    is_active: true,
                    is_king: false,
                });
                continue;
            }

            if i < 23 {
                tiles.push(PieceData::const_default());
                continue;
            }

            tiles.push(PieceData {
                is_active: true,
                color: player_color,
                is_king: false,
            });
        }

        tiles
    }

    /// Resets the board to starting state based off `player_color`
    pub fn start_new_game(&mut self, color: PieceColor) {
        self.player_color = color;
        self.pieces = Rc::new(slint::VecModel::from(Board::default_setup(color)));

        let game = self.game.unwrap();
        game.set_pieces(self.pieces.clone().into());

        self.reset_squares();
        if let Some(moves) = self.get_legal_moves() {
            let mark_indicies: Vec<usize> = moves.iter().map(|mov| mov.end).collect();
            self.mark_squares(mark_indicies.as_slice());
        }
    }

    /// Takes a `Move` struct and performs the move described within
    pub fn move_piece(&mut self, mov: Move) {
        let mut start_data = self.pieces.row_data(mov.index).unwrap();

        // Promotion to king
        start_data.is_king |= mov.promoted;

        self.pieces.set_row_data(mov.end, start_data);
        self.pieces
            .set_row_data(mov.index, PieceData::const_default());

        if let Some(captured) = mov.captured {
            for piece in captured {
                self.pieces.set_row_data(piece, PieceData::const_default())
            }
        }
    }

    /// Gives all the squares in `indices` the "marked" color
    pub fn mark_squares(&mut self, indices: &[usize]) {
        for index in indices {
            self.squares
                .set_row_data(*index, BoardSquare { marked: true });
        }
    }

    /// Turns all squares back to their original color
    pub fn reset_squares(&mut self) {
        for index in 0..32 {
            self.squares
                .set_row_data(index, BoardSquare { marked: false });
        }
    }

    /// Returns true if the `index` corresponds to an active piece on the board
    pub fn piece_is_empty(&self, index: usize) -> bool {
        assert!(index < self.pieces.row_count());
        !self.pieces.row_data(index).unwrap().is_active
    }

    /// Returns true if the `index` corresponds to a player piece on the board
    pub fn piece_is_player(&self, index: usize) -> bool {
        assert!(
            index < self.pieces.row_count(),
            "index ({}) is greater than row_count ({})",
            index,
            self.pieces.row_count()
        );
        let piece = self.pieces.row_data(index).unwrap();
        piece.color == self.player_color && piece.is_active
    }

    /// Returns true if the `index` corresponds to a non-player piece on the board
    pub fn piece_is_enemy(&self, index: usize) -> bool {
        assert!(
            index < self.pieces.row_count(),
            "index ({}) is greater than row_count ({})",
            index,
            self.pieces.row_count()
        );
        let piece = self.pieces.row_data(index).unwrap();
        piece.color != self.player_color && piece.is_active
    }

    pub fn get_player_piece_count(&self) -> u8 {
        let mut count = 0;
        for i in 0..32 {
            count += self.piece_is_player(i) as u8;
        }
        count
    }

    pub fn get_enemy_piece_count(&self) -> u8 {
        let mut count = 0;
        for i in 0..32 {
            count += self.piece_is_enemy(i) as u8;
        }
        count
    }

    pub fn get_empty_piece_count(&self) -> u8 {
        let mut count = 0;
        for i in 0..32 {
            count += self.piece_is_empty(i) as u8;
        }
        count
    }

    /// Get's all the legal moves for the given piece
    /// This works for both enemy pieces and player pieces
    pub fn get_legal_moves_piece(&self, index: usize) -> Option<(Vec<Move>, bool)> {
        assert!(index < self.pieces.row_count());
        let piece = self.pieces.row_data(index)?;
        if !piece.is_active {
            return None;
        }

        #[allow(clippy::too_many_arguments)]
        fn check_move(
            tiles: Rc<slint::VecModel<PieceData>>,
            start: usize,
            index: usize,
            local_player_color: PieceColor,
            enemy_color: PieceColor,
            is_king: bool,
            direction: &Direction,
            is_taking: bool,
        ) -> Option<(Vec<Move>, bool)> {
            // Check if the piece is on the edge of the direction
            let row_left_shifted = index % 8 < 4;
            let piece_left_side = index % 4 == 0;
            let peice_right_side = index % 4 == 3;
            if row_left_shifted && direction.is_left() && piece_left_side {
                return None;
            }

            if !row_left_shifted && direction.is_right() && peice_right_side {
                return None;
            }

            let is_local_player = local_player_color != enemy_color;
            // If the piece isn't a king it cant move backwards
            if !is_king {
                if direction.is_down() && is_local_player {
                    return None;
                }

                if direction.is_up() && !is_local_player {
                    return None;
                }
            }

            let next = index as i32 + direction.get_value(index);
            let promoting = is_local_player && next < 4 || !is_local_player && next > 32 - 4;
            if next < 0 || next > tiles.row_count() as i32 {
                return None;
            }

            let next_tile = tiles.row_data(next as usize)?;
            // If the next tile is an enemy check if the tile behind it is empty
            // If so this piece can be taken
            if next_tile.is_active {
                if next_tile.color != enemy_color || is_taking {
                    return None;
                }

                return if let Some(mut next_move) = check_move(
                    tiles,
                    start,
                    next as usize,
                    local_player_color,
                    enemy_color,
                    is_king,
                    direction,
                    true,
                ) {
                    if next_move.1 {
                        for i in 0..next_move.0.len() {
                            if next_move.0[i].captured.is_none() {
                                next_move.0.remove(i);
                            }
                        }
                    }

                    Some(next_move)
                } else {
                    None
                };
            }

            // If we are taking a piece, since the next tile is empty
            // We return the available move to take the piece
            if is_taking {
                // Check to see if we can take further pieces
                let mut further_moves = None;

                for direction in Direction::values() {
                    let moves = check_move(
                        tiles.clone(),
                        start,
                        next as usize,
                        local_player_color,
                        enemy_color,
                        is_king || promoting,
                        direction,
                        false,
                    );

                    if let Some(mut moves) = moves {
                        if !moves.1 {
                            continue;
                        }
                        for mov in &mut moves.0 {
                            unsafe { mov.captured.as_mut().unwrap_unchecked().push(index) };
                        }
                        further_moves.get_or_insert(vec![]).append(&mut moves.0);
                    }
                }

                return Some((
                    further_moves.unwrap_or(vec![Move {
                        index: start,
                        end: next as usize,
                        captured: Some(vec![index]),
                        promoted: promoting
                    }]),
                    true,
                ));
            }

            // If we aren't taking a piece, and this tile is piece is empty
            // We add this move to a list of possible moves
            let mut moves = vec![];
            let mut is_taking = false;

            // If the current piece is a king, it may be able to keep moving
            if is_king {
                if let Some(mut next_moves) = check_move(
                    tiles,
                    start,
                    next as usize,
                    local_player_color,
                    enemy_color,
                    is_king,
                    direction,
                    false,
                ) {
                    moves.append(&mut next_moves.0);
                    is_taking = next_moves.1;
                }
            }

            if !is_taking {
                moves.push(Move {
                    index: start,
                    end: next as usize,
                    captured: None,
                    promoted: promoting
                });
            }

            // Return all the available moves
            // 1 move if normal piece, x amount if king piece
            Some((moves, is_taking))
        }

        let mut moves: Option<Vec<Move>> = None;
        let mut is_taking = false;
        for direction in Direction::values() {
            // Since the direction is valid, run the check move algorithm
            let next_moves = check_move(
                self.pieces.clone(),
                index,
                index,
                self.player_color,
                piece.color.get_opposite(),
                piece.is_king,
                direction,
                false,
            );

            if let None = next_moves {
                continue;
            }

            let mut next_moves = unsafe { next_moves.unwrap_unchecked() };

            is_taking |= next_moves.1;

            if next_moves.1 == is_taking {
                moves.get_or_insert(vec![]).append(&mut next_moves.0);
            }
        }

        moves.map(|moves| {
            if !is_taking {
                return (moves, is_taking);
            }
            // Remove all non-capturing moves
            let filtered: Vec<Move> = moves
                .iter()
                .filter_map(|mov| mov.captured.as_ref().map(|_| mov.clone()))
                .collect();

            (filtered, is_taking)
        })
    }

    /// Returns all legal moves for the `player_color`
    pub fn get_legal_moves(&self) -> Option<Vec<Move>> {
        let mut moves = None;
        let mut is_taking = false;
        for index in 0..self.pieces.row_count() {
            if self.pieces.row_data(index)?.color != self.player_color {
                continue;
            }

            if let Some(mut legal_moves) = self.get_legal_moves_piece(index) {
                is_taking |= legal_moves.1;
                if legal_moves.1 == is_taking {
                    moves.get_or_insert(vec![]).append(&mut legal_moves.0);
                }
            }
        }
        moves.map(|moves| {
            if !is_taking {
                return moves;
            }

            moves
                .iter()
                .filter_map(|mov| mov.captured.as_ref().map(|_| mov.clone()))
                .collect()
        })
    }
}
