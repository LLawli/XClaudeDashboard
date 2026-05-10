use crate::window::WindowKind;

#[derive(Debug, Clone)]
pub enum Action {
    Tick,
    RevalidateWindow,
    Quit,
    RemoteFetch,
    SwitchView(WindowKind),
    Noop,
}
