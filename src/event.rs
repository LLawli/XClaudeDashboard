#[derive(Debug, Clone)]
pub enum Action {
    Tick,
    RevalidateWindow,
    Quit,
    RemoteFetch,
    Noop,
}
