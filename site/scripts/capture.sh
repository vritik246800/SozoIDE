#!/usr/bin/env bash
# Captures the screens of the landing page from a real SozoIDE (Linux build) in a neutral
# container: a demo project "orbit", user "dev", prompt "orbit $", fixed Git dates. Run it from
# the repository root; the ANSI captures land in OUT and site/scripts/screens.py turns them
# into site/src/data/screens.json.
#
#   OUT=$(mktemp -d)
#   docker run --rm -v "$PWD:/src:ro" -v sozo-target:/target -v "$OUT:/out" \
#     rust:1.97-slim bash /src/site/scripts/capture.sh
#   python3 site/scripts/screens.py "$OUT"
set -euo pipefail

apt-get update -qq
apt-get install -y -qq --no-install-recommends tmux git sqlite3 >/dev/null
# The language server of the IntelliSense capture (F26).
rustup component add rust-analyzer >/dev/null 2>&1
CARGO_TARGET_DIR=/target cargo build --locked --quiet --manifest-path /src/Cargo.toml
sozo=/target/debug/sozo

export HOME=/home/dev
project=$HOME/code/orbit
mkdir -p "$project/src" "$project/migrations"
cd "$project"

# The shells of both terminals are login shells: the prompt and the toolchain come from here.
cat >"$HOME/.bash_profile" <<'EOF'
export PATH=/usr/local/cargo/bin:$PATH
export CARGO_TARGET_DIR=/tmp/orbit-target
export PS1='orbit $ '
unset PROMPT_COMMAND HISTFILE
EOF

commit() {
  git add -A
  GIT_AUTHOR_DATE="$1" GIT_COMMITTER_DATE="$1" git commit -q -m "$2"
}

git init -q -b main
git config user.name 'Orbit Team'
git config user.email 'team@orbit.example'

cat >Cargo.toml <<'EOF'
[package]
name = "orbit"
version = "0.1.0"
edition = "2024"

[dependencies]
EOF
printf '/target\norbit.db\n' >.gitignore
cat >src/lib.rs <<'EOF'
//! Launch planning for the Orbit demo.

pub struct Launch {
    pub day: u32,
    pub crew: &'static str,
}
EOF
cargo generate-lockfile --offline --quiet
commit '2026-10-05T09:12:00Z' 'Start the Orbit crate'

cat >src/lib.rs <<'EOF'
//! Launch planning for the Orbit demo.

pub struct Launch {
    pub day: u32,
    pub crew: &'static str,
}

/// Plans one launch every seven days.
pub fn schedule(count: u32) -> Vec<Launch> {
    (1..=count)
        .map(|week| Launch { day: week * 7, crew: "Sozo" })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launches_are_a_week_apart() {
        let days: Vec<u32> = schedule(3).iter().map(|launch| launch.day).collect();
        assert_eq!(days, [7, 14, 21]);
    }
}
EOF
commit '2026-10-07T14:30:00Z' 'Plan one launch a week'

cat >src/main.rs <<'EOF'
fn main() {
    for launch in orbit::schedule(3) {
        println!("day {:>2}  crew {}", launch.day, launch.crew);
    }
}
EOF
commit '2026-10-08T10:05:00Z' 'Print the schedule'

git switch -q -c launches-table
cat >migrations/000_crews.sql <<'EOF'
CREATE TABLE crews (
    name TEXT PRIMARY KEY,
    ship TEXT NOT NULL
);
INSERT INTO crews VALUES ('Sozo', 'Aurora'), ('Vega', 'Lyra');
EOF
cat >migrations/001_launches.sql <<'EOF'
CREATE TABLE launches (
    day INTEGER PRIMARY KEY,
    crew TEXT NOT NULL REFERENCES crews (name),
    status TEXT NOT NULL
);
EOF
commit '2026-10-12T16:45:00Z' 'Add the launches table'
printf "INSERT INTO launches VALUES (7, 'Sozo', 'done'), (14, 'Sozo', 'done');\n" >migrations/002_first_launches.sql
commit '2026-10-13T11:20:00Z' 'Record the first launches'
git switch -q main
printf '# Orbit\n\nPlans one launch every seven days.\n' >README.md
commit '2026-10-14T08:40:00Z' 'Write the README'
GIT_AUTHOR_DATE='2026-10-15T09:00:00Z' GIT_COMMITTER_DATE='2026-10-15T09:00:00Z' \
  git merge -q --no-ff -m 'Merge the launches table' launches-table

# Work in progress for the Changes tab: one staged file, one modified, one new.
printf '# Orbit\n\nPlans one launch every seven days and keeps a log of each crew.\n' >README.md
sed -i 's/schedule(3)/schedule(4)/' src/main.rs
git add src/main.rs
printf "INSERT INTO launches VALUES (21, 'Sozo', 'planned'), (28, 'Vega', 'planned');\n" >migrations/003_next_launches.sql

sqlite3 orbit.db <migrations/000_crews.sql
sqlite3 orbit.db <migrations/001_launches.sql
sqlite3 orbit.db <migrations/002_first_launches.sql
sqlite3 orbit.db <migrations/003_next_launches.sql
CARGO_TARGET_DIR=/tmp/orbit-target cargo build --quiet --all-targets

tmux() { command tmux -L capture "$@"; }
keys() { tmux send-keys -t demo "$@"; sleep 0.4; }
text() { tmux send-keys -t demo -l "$1"; sleep 0.4; }
wait_for() {
  for _ in $(seq 100); do
    tmux capture-pane -p -t demo | grep -q "$1" && return 0
    sleep 0.1
  done
  echo "Timed out waiting for: $1" >&2
  tmux capture-pane -p -t demo >&2
  exit 1
}
capture() {
  sleep 0.6
  tmux capture-pane -e -p -t demo >"/out/$1.ansi"
}
# Where TEXT first is on the screen, as "column row" counted from 1 (characters, not bytes).
place() {
  tmux capture-pane -p -t demo |
    TEXT="$1" perl -CSD -ne 'my $i = index($_, $ENV{TEXT}); if ($i >= 0) { print $i + 1, " ", $., "\n"; exit }'
}
# The mouse as the terminal sends it (SGR): button 2 pressed and let go is a right-click,
# 35 a move.
mouse() {
  tmux send-keys -t demo -l $'\e'"[<$1;$2;$3M"
  [ "$1" = 35 ] || tmux send-keys -t demo -l $'\e'"[<$1;$2;$3m"
  sleep 0.4
}

tmux -f /dev/null new-session -d -s demo -x 132 -y 34 -c "$project" \
  "env TERM=xterm-256color SHELL=/bin/bash SOZO_REDUCED_MOTION=1 $sozo"
wait_for 'Press any key'
keys Space
wait_for 'Explorer'
keys Down
keys Enter
keys Down
keys Enter
wait_for 'Launch planning'
keys C-j
wait_for 'orbit \$'
text 'cargo test -q --lib'
keys Enter
wait_for 'test result'
capture editor
# Ctrl+R from the editor runs the Cargo project in the bottom terminal (F25): Ctrl+J gives the
# focus back to the editor first.
keys C-j
keys C-r
wait_for 'crew Sozo'
capture run

keys C-g
wait_for 'Staged'
capture git-changes
keys Tab
wait_for 'Merge the launches table'
# The timeline with the files of the merge slid in from the right.
keys Enter
wait_for 'Enter: diff'
capture git-graph
keys Escape
keys C-g

keys C-d
wait_for 'Add Connection'
text 'n'
text 'orbit'
keys Tab
keys Tab
keys Tab
text 'orbit.db'
keys Enter
# Connecting opens the diagram: both tables in, the crew name and the launch day ticked.
wait_for 'Tables (2)'
keys Space
keys Down
keys Space
keys Right
keys Enter
keys Down
keys Down
keys Enter
wait_for 'launches.crew'
capture db-query
# The builder: launches counted per crew, the busiest first, then run.
keys Tab
keys Down
keys Right
keys Enter
keys Enter
keys Right
keys Enter
keys Enter
keys F5
wait_for 'count_day'
capture db-results
keys C-d

keys C-p
text 'launch'
wait_for '◂ 1/'
capture search
keys Escape
sleep 0.4
keys F1
wait_for 'Help - all commands'
capture help
keys Escape
# Last, as it edits src/lib.rs: rust-analyzer suggests the fields of a launch and underlines
# the line that is not finished yet (F26). Ctrl+J closes the terminal of the run capture.
keys C-j
for _ in $(seq 30); do tmux send-keys -t demo Down; done
keys End
keys Enter
keys Enter
text 'pub fn first() -> u32 {'
keys Enter
text '    schedule(1)[0].'
wait_for ' 1/'
capture intellisense
# The right-click menu on the call of schedule, the mouse on Find References: the answers in
# the small box beside the menu (F31, Q73).
keys Escape
read -r column row < <(place 'schedule(1)')
mouse 2 "$((column + 2))" "$row"
wait_for 'Find References'
read -r column row < <(place 'Find References')
mouse 35 "$((column + 3))" "$row"
wait_for 'References ('
capture menu
tmux kill-server
echo "Captured $(ls /out | wc -l) screens into /out"
