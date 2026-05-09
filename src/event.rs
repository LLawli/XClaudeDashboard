#[derive(Debug, Clone)]
pub enum Action {
    Tick,
    Quit,
    RemoteFetch,
    Noop,
}
