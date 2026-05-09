use std::path::PathBuf;
use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use futures::StreamExt;
use rusqlite::Connection;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::time;

use crate::aggregate::WindowAggregate;
use crate::cli::Cli;
use crate::colors::ColorMap;
use crate::config;
use crate::db;
use crate::event::Action;
use crate::pricing::{self, PricingCache};
use crate::rate::RateState;
use crate::remote::{self, SyncOutcome};
use crate::tui::Tui;
use crate::ui;
use crate::window::Window;

pub const RATE_WINDOW_MIN: u32 = 15;
pub const IDLE_THRESHOLD_PER_MIN: f64 = 100.0;
const PRICES_TTL_HOURS: u32 = 24;
const WINDOW_REVALIDATE_INTERVAL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Bootstrap,
    Active,
    Closed,
}

#[derive(Debug, Clone)]
pub enum FooterStatus {
    Idle,
    SyncingRemote,
    SyncingPrices,
    SyncedRemote { pulled: usize, pushed: usize },
    SyncedPrices { models: usize },
    Error(String),
}

enum Bg {
    Pricing(Result<PricingCache>),
    Remote(Result<SyncOutcome>),
}

pub struct App {
    pub should_quit: bool,
    pub status: Status,
    pub now_secs: i64,
    pub tick_ms: u64,

    pub db_path: PathBuf,
    pub cloud_config_path: PathBuf,
    pub prices_path: PathBuf,

    pub last_data_version: Option<i64>,
    pub window: Option<Window>,
    pub aggregate: WindowAggregate,
    pub rate: RateState,
    pub colors: ColorMap,
    pub pricing: PricingCache,

    pub fetching_remote: bool,
    pub fetching_prices: bool,
    pub footer: FooterStatus,

    db: Option<Connection>,
    bg_tx: Option<UnboundedSender<Bg>>,
}

impl App {
    pub fn new(args: Cli) -> Result<Self> {
        let db_path = match args.db_path.clone() {
            Some(p) => p,
            None => config::default_db_path()?,
        };
        let cloud_config_path = match args.cloud_config.clone() {
            Some(p) => p,
            None => config::default_cloud_config()?,
        };
        let prices_path = config::default_prices_cache()?;

        let pricing = PricingCache::load_from_disk(&prices_path).unwrap_or_default();

        Ok(Self {
            should_quit: false,
            status: Status::Bootstrap,
            now_secs: now_secs(),
            tick_ms: args.tick_ms,
            db_path,
            cloud_config_path,
            prices_path,
            last_data_version: None,
            window: None,
            aggregate: WindowAggregate::default(),
            rate: RateState::new(),
            colors: ColorMap::new(),
            pricing,
            fetching_remote: false,
            fetching_prices: false,
            footer: FooterStatus::Idle,
            db: None,
            bg_tx: None,
        })
    }

    pub async fn run(&mut self, terminal: &mut Tui) -> Result<()> {
        let (tx, mut rx) = unbounded_channel::<Bg>();
        self.bg_tx = Some(tx);

        // Initial state from disk + decide whether to fetch pricing.
        self.now_secs = now_secs();
        if let Err(e) = self.refresh_from_db() {
            self.footer = FooterStatus::Error(format!("db read: {e}"));
        }
        self.update_status();
        self.maybe_spawn_pricing_fetch(/* force */ false);

        let mut events = EventStream::new();
        let mut tick = time::interval(Duration::from_millis(self.tick_ms));
        tick.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        let mut revalidate = time::interval(WINDOW_REVALIDATE_INTERVAL);
        revalidate.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        // Skip the immediate first fire of `revalidate` (we just refreshed).
        let _ = revalidate.tick().await;

        terminal.draw(|f| ui::render(f, self))?;

        while !self.should_quit {
            let action = tokio::select! {
                _ = tick.tick() => Action::Tick,
                _ = revalidate.tick() => Action::RevalidateWindow,
                maybe_event = events.next() => match maybe_event {
                    Some(Ok(ev)) => self.translate_event(ev),
                    Some(Err(e)) => return Err(e.into()),
                    None => Action::Quit,
                },
                Some(msg) = rx.recv() => {
                    self.handle_bg(msg);
                    Action::Noop
                }
                _ = tokio::signal::ctrl_c() => Action::Quit,
            };

            self.update(action)?;
            terminal.draw(|f| ui::render(f, self))?;
        }
        Ok(())
    }

    fn translate_event(&self, ev: Event) -> Action {
        let Event::Key(key) = ev else {
            return Action::Noop;
        };
        if key.kind != KeyEventKind::Press {
            return Action::Noop;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Char('r') => Action::RemoteFetch,
            _ => Action::Noop,
        }
    }

    fn update(&mut self, action: Action) -> Result<()> {
        self.now_secs = now_secs();
        match action {
            Action::Quit => self.should_quit = true,
            Action::Tick => self.tick()?,
            Action::RevalidateWindow => {
                if let Err(e) = self.refresh_window_only() {
                    self.footer = FooterStatus::Error(format!("window read: {e}"));
                }
                self.update_status();
            }
            Action::RemoteFetch => self.spawn_remote_sync(),
            Action::Noop => {}
        }
        Ok(())
    }

    fn tick(&mut self) -> Result<()> {
        let dv_changed = self.check_data_version()?;
        if dv_changed {
            if let Err(e) = self.refresh_from_db() {
                self.footer = FooterStatus::Error(format!("db refresh: {e}"));
            } else {
                self.maybe_spawn_pricing_fetch(/* force */ false);
            }
        }
        self.update_status();
        Ok(())
    }

    fn check_data_version(&mut self) -> Result<bool> {
        let dv = {
            let conn = self.conn()?;
            db::data_version(conn)?
        };
        let changed = self.last_data_version != Some(dv);
        self.last_data_version = Some(dv);
        Ok(changed)
    }

    fn refresh_from_db(&mut self) -> Result<()> {
        let (window, aggregate, samples) = {
            let conn = self.conn()?;
            let window = Window::current(conn)?;
            let (agg, samples) = if let Some(w) = window {
                (
                    WindowAggregate::fetch(conn, w.start_at, w.resets_at)?,
                    crate::aggregate::output_samples(conn, w.start_at, w.resets_at)?,
                )
            } else {
                (WindowAggregate::default(), Vec::new())
            };
            (window, agg, samples)
        };

        self.window = window;
        if window.is_some() {
            for model in aggregate.per_model.keys() {
                self.colors.assign(model);
            }
            self.aggregate = aggregate;
            self.rate.replace_from_samples(samples);
        } else {
            self.aggregate = WindowAggregate::default();
            self.rate = RateState::new();
            self.colors.reset();
        }
        Ok(())
    }

    fn refresh_window_only(&mut self) -> Result<()> {
        let window = {
            let conn = self.conn()?;
            Window::current(conn)?
        };
        self.window = window;
        Ok(())
    }

    fn update_status(&mut self) {
        let prev = self.status;
        let new = next_status(prev, self.window.as_ref(), self.now_secs);
        if new != prev {
            // Window flipped — colors are scoped per-window per the design.
            if matches!(new, Status::Active) && matches!(prev, Status::Closed) {
                self.colors.reset();
            }
        }
        self.status = new;
    }

    fn maybe_spawn_pricing_fetch(&mut self, force: bool) {
        if self.fetching_prices {
            return;
        }
        let should_fetch = force
            || self.pricing.models.is_empty()
            || self.pricing.is_stale(self.now_secs, PRICES_TTL_HOURS)
            || self
                .aggregate
                .per_model
                .keys()
                .any(|m| !self.pricing.has(m));
        if !should_fetch {
            return;
        }
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.fetching_prices = true;
        self.footer = FooterStatus::SyncingPrices;
        tokio::spawn(async move {
            let res = pricing::fetch_from_litellm().await;
            let _ = tx.send(Bg::Pricing(res));
        });
    }

    fn spawn_remote_sync(&mut self) {
        if self.fetching_remote {
            return;
        }
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let db_path = self.db_path.clone();
        let cloud_config_path = self.cloud_config_path.clone();
        self.fetching_remote = true;
        self.footer = FooterStatus::SyncingRemote;
        tokio::spawn(async move {
            let res = async move {
                let cfg = config::load_cloud_config(&cloud_config_path)?;
                remote::sync_turso(&db_path, &cfg).await
            }
            .await;
            let _ = tx.send(Bg::Remote(res));
        });
    }

    fn handle_bg(&mut self, msg: Bg) {
        match msg {
            Bg::Pricing(Ok(cache)) => {
                let count = cache.models.len();
                if let Err(e) = cache.save_to_disk(&self.prices_path) {
                    tracing::warn!("failed to save pricing cache: {e}");
                }
                self.pricing = cache;
                self.fetching_prices = false;
                self.footer = FooterStatus::SyncedPrices { models: count };
            }
            Bg::Pricing(Err(e)) => {
                self.fetching_prices = false;
                self.footer = FooterStatus::Error(format!("pricing: {e}"));
            }
            Bg::Remote(Ok(out)) => {
                self.fetching_remote = false;
                self.footer = FooterStatus::SyncedRemote {
                    pulled: out.pulled_rows,
                    pushed: out.pushed_rows,
                };
                // Force a re-read on next tick — the sync wrote to local SQLite,
                // and `data_version` should already reflect that. The next Tick
                // will pick it up automatically.
            }
            Bg::Remote(Err(e)) => {
                self.fetching_remote = false;
                self.footer = FooterStatus::Error(format!("sync: {e}"));
            }
        }
    }

    fn conn(&mut self) -> Result<&Connection> {
        if self.db.is_none() {
            self.db = Some(db::open(&self.db_path)?);
        }
        Ok(self.db.as_ref().unwrap())
    }
}

/// Pure state-transition: `Active` iff there's a window whose `resets_at` is
/// still in the future; otherwise `Closed`. Bootstrap collapses into one of the two
/// based on the same predicate.
pub fn next_status(prev: Status, window: Option<&Window>, now_secs: i64) -> Status {
    let _ = prev; // currently a function of `window` + `now`, kept arg for future asymmetric rules
    match window {
        Some(w) if w.resets_at > now_secs => Status::Active,
        _ => Status::Closed,
    }
}

pub fn now_secs() -> i64 {
    ::time::OffsetDateTime::now_utc().unix_timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(resets_at: i64) -> Window {
        Window {
            start_at: resets_at - 18_000,
            resets_at,
            used_percentage: 50.0,
            updated_at: 0,
        }
    }

    #[test]
    fn next_status_bootstrap_to_active_when_window_in_future() {
        let w = win(1_000);
        assert_eq!(
            next_status(Status::Bootstrap, Some(&w), 500),
            Status::Active
        );
    }

    #[test]
    fn next_status_bootstrap_to_closed_when_no_window() {
        assert_eq!(next_status(Status::Bootstrap, None, 0), Status::Closed);
    }

    #[test]
    fn next_status_bootstrap_to_closed_when_window_past() {
        let w = win(100);
        assert_eq!(
            next_status(Status::Bootstrap, Some(&w), 200),
            Status::Closed
        );
    }

    #[test]
    fn next_status_active_to_closed_when_now_reaches_reset() {
        let w = win(100);
        assert_eq!(next_status(Status::Active, Some(&w), 100), Status::Closed);
        assert_eq!(next_status(Status::Active, Some(&w), 101), Status::Closed);
    }

    #[test]
    fn next_status_closed_to_active_when_new_window_appears() {
        let w = win(2_000);
        assert_eq!(next_status(Status::Closed, Some(&w), 1_000), Status::Active);
    }

    #[test]
    fn next_status_closed_stays_closed_when_window_still_past() {
        let w = win(100);
        assert_eq!(next_status(Status::Closed, Some(&w), 500), Status::Closed);
    }
}
