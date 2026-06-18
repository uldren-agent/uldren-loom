use anyhow::{bail, Result};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct Pos {
    pub x: usize,
    pub y: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Dir {
    N,
    E,
    S,
    W,
}

impl Dir {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "N" | "NORTH" | "UP" => Some(Self::N),
            "E" | "EAST" | "RIGHT" => Some(Self::E),
            "S" | "SOUTH" | "DOWN" => Some(Self::S),
            "W" | "WEST" | "LEFT" => Some(Self::W),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::N => "N",
            Self::E => "E",
            Self::S => "S",
            Self::W => "W",
        }
    }
}

#[derive(Clone)]
pub struct Maze {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
    entrance: Pos,
    exit: Pos,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Wall,
    Open,
}

impl Maze {
    pub fn generate(width: usize, height: usize, seed: u64) -> Result<Self> {
        if width < 2 || height < 2 {
            bail!("maze dimensions must be at least 2");
        }
        if width < 7 || height < 7 {
            return Ok(Self::open_grid(width, height));
        }
        if width % 2 == 0 || height % 2 == 0 {
            bail!("maze dimensions 7 and above must be odd");
        }

        let entrance = Pos {
            x: 0,
            y: height - 1,
        };
        let exit = Pos { x: width - 1, y: 0 };
        let mut maze = Self {
            width,
            height,
            cells: vec![Cell::Wall; width * height],
            entrance,
            exit,
        };

        let mut rng = Rng::new(seed ^ ((width as u64) << 32) ^ height as u64);
        let start = Pos {
            x: 1,
            y: height - 2,
        };
        maze.set(start, Cell::Open);

        let mut stack = vec![start];
        while let Some(current) = stack.pop() {
            let mut neighbors = maze.unvisited_neighbors(current);
            rng.shuffle(&mut neighbors);
            if let Some(next) = neighbors.pop() {
                stack.push(current);
                let wall = Pos {
                    x: (current.x + next.x) / 2,
                    y: (current.y + next.y) / 2,
                };
                maze.set(wall, Cell::Open);
                maze.set(next, Cell::Open);
                stack.push(next);
            }
        }

        maze.set(entrance, Cell::Open);
        maze.set(
            Pos {
                x: 1,
                y: height - 1,
            },
            Cell::Open,
        );
        maze.set(exit, Cell::Open);
        maze.set(Pos { x: width - 2, y: 0 }, Cell::Open);

        Ok(maze)
    }

    fn open_grid(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![Cell::Open; width * height],
            entrance: Pos {
                x: 0,
                y: height - 1,
            },
            exit: Pos { x: width - 1, y: 0 },
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn entrance(&self) -> Pos {
        self.entrance
    }

    pub fn exit(&self) -> Pos {
        self.exit
    }

    pub fn is_open(&self, pos: Pos) -> bool {
        pos.x < self.width && pos.y < self.height && self.cells[self.idx(pos)] == Cell::Open
    }

    pub fn step(&self, pos: Pos, dir: Dir) -> Option<Pos> {
        let next = match dir {
            Dir::N if pos.y > 0 => Pos {
                x: pos.x,
                y: pos.y - 1,
            },
            Dir::E if pos.x + 1 < self.width => Pos {
                x: pos.x + 1,
                y: pos.y,
            },
            Dir::S if pos.y + 1 < self.height => Pos {
                x: pos.x,
                y: pos.y + 1,
            },
            Dir::W if pos.x > 0 => Pos {
                x: pos.x - 1,
                y: pos.y,
            },
            _ => return None,
        };
        self.is_open(next).then_some(next)
    }

    pub fn shortest_path(&self, start: Pos) -> Option<Vec<Dir>> {
        let mut queue = VecDeque::from([start]);
        let mut came_from: HashMap<Pos, (Pos, Dir)> = HashMap::new();
        let mut seen = vec![false; self.width * self.height];
        seen[self.idx(start)] = true;

        while let Some(pos) = queue.pop_front() {
            if pos == self.exit {
                break;
            }
            for dir in [Dir::N, Dir::E, Dir::S, Dir::W] {
                if let Some(next) = self.step(pos, dir) {
                    let idx = self.idx(next);
                    if !seen[idx] {
                        seen[idx] = true;
                        came_from.insert(next, (pos, dir));
                        queue.push_back(next);
                    }
                }
            }
        }

        if start == self.exit {
            return Some(Vec::new());
        }
        if !came_from.contains_key(&self.exit) {
            return None;
        }

        let mut cursor = self.exit;
        let mut rev = Vec::new();
        while cursor != start {
            let (prev, dir) = came_from[&cursor];
            rev.push(dir);
            cursor = prev;
        }
        rev.reverse();
        Some(rev)
    }

    pub fn shortest_distance(&self, start: Pos) -> Option<usize> {
        self.shortest_path(start).map(|path| path.len())
    }

    pub fn char_at(&self, pos: Pos) -> char {
        if pos == self.entrance {
            'A'
        } else if pos == self.exit {
            'X'
        } else if self.is_open(pos) {
            '.'
        } else {
            '#'
        }
    }

    fn idx(&self, pos: Pos) -> usize {
        pos.y * self.width + pos.x
    }

    fn set(&mut self, pos: Pos, cell: Cell) {
        let idx = self.idx(pos);
        self.cells[idx] = cell;
    }

    fn unvisited_neighbors(&self, pos: Pos) -> Vec<Pos> {
        let mut out = Vec::new();
        let candidates = [
            (pos.x, pos.y.wrapping_sub(2), pos.y >= 3),
            (pos.x + 2, pos.y, pos.x + 2 < self.width - 1),
            (pos.x, pos.y + 2, pos.y + 2 < self.height - 1),
            (pos.x.wrapping_sub(2), pos.y, pos.x >= 3),
        ];
        for (x, y, valid) in candidates {
            if valid {
                let next = Pos { x, y };
                if self.cells[self.idx(next)] == Cell::Wall {
                    out.push(next);
                }
            }
        }
        out
    }
}

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn shuffle<T>(&mut self, values: &mut [T]) {
        for i in (1..values.len()).rev() {
            let j = (self.next() as usize) % (i + 1);
            values.swap(i, j);
        }
    }
}
