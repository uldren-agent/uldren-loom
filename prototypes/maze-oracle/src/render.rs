use crate::maze::{Maze, Pos};
use crate::run::Visibility;

pub struct VisibleMaze {
    pub ascii: String,
    pub window: Window,
}

#[derive(Clone, Copy, serde::Serialize)]
pub struct Window {
    pub x_min: usize,
    pub y_min: usize,
    pub x_max: usize,
    pub y_max: usize,
}

pub fn render_visible(maze: &Maze, rat: Pos, visibility: Visibility) -> VisibleMaze {
    let window = window_for(maze, rat, visibility);
    let mut out = String::new();
    out.push_str("    ");
    for x in window.x_min..=window.x_max {
        out.push_str(&(x % 10).to_string());
    }
    out.push('\n');
    for y in window.y_min..=window.y_max {
        out.push_str(&format!("{y:>3} "));
        for x in window.x_min..=window.x_max {
            let pos = Pos { x, y };
            if pos == rat {
                out.push('R');
            } else {
                out.push(maze.char_at(pos));
            }
        }
        out.push('\n');
    }
    VisibleMaze { ascii: out, window }
}

fn window_for(maze: &Maze, rat: Pos, visibility: Visibility) -> Window {
    match visibility {
        Visibility::Full => Window {
            x_min: 0,
            y_min: 0,
            x_max: maze.width() - 1,
            y_max: maze.height() - 1,
        },
        Visibility::Percent(percent) => {
            let side = side_for(maze.width().max(maze.height()), percent);
            let radius = side / 2;
            let x_min = rat.x.saturating_sub(radius);
            let y_min = rat.y.saturating_sub(radius);
            let x_max = (rat.x + radius).min(maze.width() - 1);
            let y_max = (rat.y + radius).min(maze.height() - 1);
            Window {
                x_min,
                y_min,
                x_max,
                y_max,
            }
        }
    }
}

fn side_for(size: usize, percent: u8) -> usize {
    let mut side = ((size * percent as usize) + 99) / 100;
    if side % 2 == 0 {
        side += 1;
    }
    side.max(3).min(size)
}
