# Chores-01

Discussions and notes on various chores in github compatible markdown.
There is also a [todo.md](todo.md) file and it tracks tasks and in
general there should be a chore section for each task with the why
and how this task will be completed.

See [Chores format](README.md#chores-format)

## A completed dummy chore (20260322 0.1.0)

A completed dummy chore description.

## A dummy chore (TBD)

A dummy chore description.

## Create initial site (0.1.0)

Created the `site/` directory with a single-page website for no-wars.org.
The page displays "Do not attack Iran". Added `site/config.toml` for
version tracking. Using `miniserve` (Rust) as the local dev server.

## Add visitor counter and voting (0.2.0)

Replaced miniserve with a custom Axum backend (`src/main.rs`) that serves
static files from `site/` and provides API endpoints for visitor tracking
and voting. Uses SQLite (rusqlite) for persistence.

- Unique visitors identified by cookie UUID + IP/UA fingerprint fallback
- Each visitor gets one vote (thumbs up/down), toggle to remove
- Config in `site/config.toml` (bind address, port, db path)
- DB stored in `data/` (gitignored)

## Add systemd user service (TBD)

Create a systemd user service (`~/.config/systemd/user/no-wars.service`)
for running the server. Restart only needed for Rust code or config
changes; static file edits (HTML/CSS) are served live — just refresh
the browser.
